use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::fmt::Debug;

use futures::compat::*;
use futures::task::Context;
use futures::channel::mpsc::{unbounded, UnboundedSender, UnboundedReceiver};
use futures::{
    future, join, try_join, ready, try_ready, Future, FutureExt, Poll, SinkExt, Stream, StreamExt, TryFutureExt,
};

use tokio::codec::{BytesCodec, Framed};
use tokio::io::{AsyncRead, AsyncWrite, Read, Write, ReadHalf, WriteHalf};
use tokio::net::tcp::Incoming;
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::TaskExecutor;

use http_muncher::{Parser, ParserHandler};

use fut_pool::Pool;
use fut_pool::tcp::TcpConnection;

use crate::builder::{ProxyBuilder};
use crate::socket::Socket;
use crate::poll_future_01_in_03;
use crate::util::tracer;
use crate::adapter::{Adapter, AdapterAction};

use smallvec::SmallVec;
use std::time::Duration;
use bytes::{BytesMut, Bytes};
use parking_lot::Mutex;

use std::marker::PhantomData;
use crate::adapter::http::HttpAdapter;
use crate::adapter::passthrough::PassthroughAdapter;

pub type ProxyEvent<A: Adapter> = Result<(A::IncomingOutput, A::OutgoingOutput), ProxyError<A::Error>>;
pub type ProxyEventReceiver<A: Adapter> = UnboundedReceiver<ProxyEvent<A>>;
pub type ProxyEventReceiverHttp = ProxyEventReceiver<HttpAdapter>;
pub type ProxyEventReceiverPassthrough = ProxyEventReceiver<PassthroughAdapter>;

type ProxyEventSender<A: Adapter> = UnboundedSender<ProxyEvent<A>>;

pub type ProxyPassthrough = Proxy<PassthroughAdapter>;
pub type ProxyHttp = Proxy<HttpAdapter>;

pub struct Proxy<A: Adapter + 'static>
{
    pub(crate) bind_addr: SocketAddr,
    pub(crate) remote_addr: SocketAddr,
    pub(crate) pool: Pool<TcpConnection>,
    pub(crate) events: Option<ProxyEventSender<A>>,
}

impl<A: Adapter> Proxy<A>
{
    pub fn builder() -> ProxyBuilder<A> {
        ProxyBuilder::new()
    }

    pub fn accept_events(&mut self) -> ProxyEventReceiver<A> {
        if self.events.is_none() {
            let (s, r) = unbounded();
            self.events = Some(s);
            r
        } else {
            panic!("events already consumed")
        }
    }

    pub fn listen(self, executor: TaskExecutor) -> io::Result<()> {
        let remote_addr = self.remote_addr;
        let pool = self.pool.clone();

        let listener = TcpListener::bind(&self.bind_addr)?;
        let server = listener.incoming();

        let events_sender = self.events.clone();
        let incoming_future = handle_incoming::<A>(
            executor.clone(),
            server,
            events_sender,
            pool,
        );
        executor.spawn(
            incoming_future
                .unit_error()
                .boxed()
                .compat()
        );

        Ok(())
    }
}

async fn handle_incoming<A: Adapter + 'static>(
    mut executor: TaskExecutor,
    server: Incoming,
    mut events_sender: Option<ProxyEventSender<A>>,
    pool: Pool<TcpConnection>,
) {
    let mut server = server.compat();

    while let Some(raw_incoming_socket) = server.next().await {
        let raw_incoming_socket: TcpStream = match raw_incoming_socket {
            Ok(socket) => socket,
            Err(err) => {
                error!("error incoming socket {}", err);
                continue
            }
        };

        let mut s = events_sender.clone();
        let incoming_socket = Socket::new(raw_incoming_socket);
        let pool = pool.clone();
        let mut event_sender = events_sender.clone();

        let socket_future = async move {
            let res = socket_handler::<A>(incoming_socket, pool).await;

            if let Some(sender) = &mut event_sender {
                sender.send(res).await.unwrap();
            }
        };

        executor.spawn(
            socket_future
                .unit_error()
                .boxed()
                .compat()
        );
    }
}

async fn socket_handler<A: Adapter + 'static>(
    incoming_socket: Socket,
    pool: Pool<TcpConnection>,
) -> Result<(A::IncomingOutput, A::OutgoingOutput), ProxyError<A::Error>>
{
    let mut local_socket = pool.take_unguarded().await?;

    let local_socket = Socket::new(local_socket.0);

    let (local_read, local_write) = local_socket.clone().split();
    let (incoming_read, incoming_write) = incoming_socket.split();

    let incoming_to_local = Pipe::new(
        incoming_read,
        local_write,
        A::default(),
        PipeType::Incoming,
    );
    let local_to_incoming = Pipe::new(
        local_read,
        incoming_write,
        A::default(),
        PipeType::Outgoing,
    );

    let ((request, _), (_, response)) = try_join!(incoming_to_local, local_to_incoming)?;

    let tcp_connection = TcpConnection::new(local_socket.into_inner());
    pool.put(tcp_connection);

    Ok((request.unwrap(), response.unwrap()))
}

#[derive(Debug)]
pub enum ProxyError<T> {
    Io(io::Error),
    Custom(T)
}

impl<T> From<io::Error> for ProxyError<T> {
    fn from(e: io::Error) -> Self {
        ProxyError::Io(e)
    }
}

macro_rules! poll_future_pipe {
    ($e:expr) => {
        match $e {
            Ok(futures01::Async::Ready(b)) => b,
            Ok(futures01::Async::NotReady) => return Poll::Pending,
            Err(err) => return Poll::Ready(Err(ProxyError::Io(err))),
        }
    };
}

macro_rules! match_adapter_result {
    ($e:expr, $w:expr, $data:expr) => {
        match $e {
            Ok(res) => match res {
                AdapterAction::WriteAndEnd(val) => {
                    poll_future_pipe!($w.poll_write($data));
                    Some(val)
                },
                AdapterAction::Write => {
                    poll_future_pipe!($w.poll_write($data));
                    None
                },
                AdapterAction::End(val) => Some(val)
            },
            Err(err) => return Poll::Ready(Err(ProxyError::Custom(err))),
        }
    };
}

#[derive(Clone)]
enum PipeType {
    Incoming,
    Outgoing,
}

#[derive(Debug)]
enum PipeState<A: Adapter> {
    Read,
    Handle(BytesMut),
    Write(
        Bytes,
        Option<Result<AdapterAction<A::IncomingOutput>, A::Error>>,
        Option<Result<AdapterAction<A::OutgoingOutput>, A::Error>>,
    ),
}

use atomic_refcell::AtomicRefCell;

struct Pipe<A: Adapter>
{
    r: ReadHalf<Socket>,
    w: WriteHalf<Socket>,
    a: A,
    t: PipeType,
    state: Arc<AtomicRefCell<PipeState<A>>>,
}

impl<A: Adapter> Pipe<A>
{
    fn new(r: ReadHalf<Socket>, w: WriteHalf<Socket>, a: A, t: PipeType) -> Self {
        let state = Arc::new(AtomicRefCell::new(PipeState::Read));
        Pipe {r, w, a, t, state }
    }
}

impl<A: Adapter> Future for Pipe<A>
{
    type Output = Result<(Option<A::IncomingOutput>, Option<A::OutgoingOutput>), ProxyError<A::Error>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let pipe_type = self.t.clone();
        let state = self.state.clone();
        let mut state = state.borrow_mut();

        match *state {
            PipeState::Read => {
                let mut buf = BytesMut::new();
                buf.reserve(self.a.buffer_size());
                let n_read = poll_future_pipe!(self.r.read_buf(&mut buf));
                unsafe { buf.set_len(n_read) };
                *state = PipeState::Handle(buf);
                cx.waker().wake_by_ref();
                Poll::Pending
            },
            PipeState::Handle(ref mut buf) => {
                //TODO: I would love to simplify this or make it more readable and understandable
                let (mut result_incoming, mut result_outgoing) = match pipe_type {
                    PipeType::Incoming => (Some(ready!(self.a.poll_handle_incoming(cx, buf))), None),
                    PipeType::Outgoing => (None, Some(ready!(self.a.poll_handle_outgoing(cx, buf)))),
                };

                *state = PipeState::Write(buf.take().freeze(), result_incoming.take(), result_outgoing.take());
                cx.waker().wake_by_ref();
                Poll::Pending
            },
            PipeState::Write(ref buf, ref mut result_incoming, ref mut result_outgoing) => {
                match pipe_type {
                    PipeType::Incoming => match match_adapter_result!(result_incoming.take().unwrap(), self.w, &buf[..]) {
                        Some(val) => {
                            Poll::Ready(Ok((Some(val), None)))
                        },
                        None => {
                            cx.waker().wake_by_ref();
                            *state = PipeState::Read;
                            Poll::Pending
                        }
                    },
                    PipeType::Outgoing => match match_adapter_result!(result_outgoing.take().unwrap(), self.w, &buf[..]) {
                        Some(val) => {
                            Poll::Ready(Ok((None, Some(val))))
                        },
                        None => {
                            cx.waker().wake_by_ref();
                            *state = PipeState::Read;
                            Poll::Pending
                        }
                    }
                }
            },
        }
    }
}

#[derive(Default)]
struct HttpParserHandler {
    done: bool,
}

impl ParserHandler for HttpParserHandler {
    #[inline]
    fn on_message_complete(&mut self, _parser: &mut Parser) -> bool {
        self.done = true;
        true
    }
}

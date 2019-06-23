use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use futures::compat::*;
use futures::task::Context;
use futures::channel::mpsc::{unbounded, UnboundedSender, UnboundedReceiver};
use futures::{
    future, join, try_join, ready, Future, FutureExt, Poll, SinkExt, Stream, StreamExt, TryFutureExt,
};

use tokio::codec::{BytesCodec, Framed};
use tokio::io::{AsyncRead, AsyncWrite, Read, Write, ReadHalf, WriteHalf};
use tokio::net::tcp::Incoming;
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::TaskExecutor;

use http_muncher::{Parser, ParserHandler};

use fut_pool::Pool;
use fut_pool::tcp::TcpConnection;

use crate::builder::{ProxyBuilder, AdapterFactory};
use crate::socket::Socket;
use crate::poll_future_01_in_03;
use crate::util::tracer;
use crate::adapter::Adapter;

use smallvec::SmallVec;
use std::time::Duration;
use bytes::BytesMut;
use parking_lot::Mutex;

use std::marker::PhantomData;

pub type ProxyEvent = Result<(), ((), io::Error)>;
pub type ProxyEventReceiver = UnboundedReceiver<ProxyEvent>;
type ProxyEventSender = UnboundedSender<ProxyEvent>;

pub struct Proxy<A: Adapter + 'static>
    where
        <A as Adapter>::Output: Send,
        <A as Adapter>::Error: Send,
{
    pub(crate) bind_addr: SocketAddr,
    pub(crate) remote_addr: SocketAddr,
    pub(crate) pool: Pool<TcpConnection>,
    pub(crate) adapter: PhantomData<A>,
}

impl<A: Adapter + 'static> Proxy<A>
    where
        <A as Adapter>::Output: Send,
        <A as Adapter>::Error: Send,
{
    pub fn builder() -> ProxyBuilder<A> {
        ProxyBuilder::new()
    }

    pub fn bind(&self, executor: TaskExecutor) -> io::Result<ProxyEventReceiver> {
        let remote_addr = self.remote_addr;
        let pool = self.pool.clone();

        let listener = TcpListener::bind(&self.bind_addr)?;
        let server = listener.incoming();

        let (s, r) = unbounded();
        let incoming_future = handle_incoming::<A>(executor.clone(), server, s, pool);
        executor.spawn(
            incoming_future
                .unit_error()
                .boxed()
                .compat()
        );

        Ok(r)
    }
}

async fn handle_incoming<A: Adapter + 'static>(
    mut executor: TaskExecutor,
    server: Incoming,
    event_sender: ProxyEventSender,
    pool: Pool<TcpConnection>,
)
    where
        <A as Adapter>::Output: Send,
        <A as Adapter>::Error: Send,
{
    let mut server = server.compat();

    pool.initialize(100).await.unwrap();

    while let Some(raw_incoming_socket) = server.next().await {
        let raw_incoming_socket: TcpStream = raw_incoming_socket.unwrap();
        raw_incoming_socket.set_nodelay(true).unwrap();
        raw_incoming_socket.set_linger(None).unwrap();
        raw_incoming_socket.set_keepalive(Some(Duration::from_secs(30))).unwrap();

        let mut s = event_sender.clone();
        let incoming_socket = Socket::new(raw_incoming_socket);
        let pool = pool.clone();

        let socket_future = async move {
            let res = socket_handler::<A>(incoming_socket.clone(), pool).await;
            match res {
                Ok(_) => s.send(Ok(())),
                Err(err) => s.send(Err(((), err)))
            }.await.expect("sending request/response result to channel");
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
) -> io::Result<()>
    where
        <A as Adapter>::Output: Send,
        <A as Adapter>::Error: Send,
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

    try_join!(incoming_to_local, local_to_incoming);

    let tcp_connection = TcpConnection::new(local_socket.into_inner());
    pool.put(tcp_connection);

    Ok(())
}

enum PipeError<T> {
    Io(io::Error),
    Custom(T)
}

impl<T> From<io::Error> for PipeError<T> {
    fn from(e: io::Error) -> Self {
        PipeError::Io(e)
    }
}

enum PipeType {
    Incoming,
    Outgoing,
}

struct Pipe<A: Adapter>
    where
        <A as Adapter>::Output: Send,
        <A as Adapter>::Error: Send,
{
    r: ReadHalf<Socket>,
    w: WriteHalf<Socket>,
    a: A,
    t: PipeType,
}

impl<A: Adapter> Pipe<A>
    where
        <A as Adapter>::Output: Send,
        <A as Adapter>::Error: Send,
{
    fn new(r: ReadHalf<Socket>, w: WriteHalf<Socket>, a: A, t: PipeType) -> Self {
        Pipe {r, w, a, t}
    }
}

macro_rules! poll_future_pipe {
    ($e:expr) => {
        match $e {
            Ok(futures01::Async::Ready(b)) => b,
            Ok(futures01::Async::NotReady) => return Poll::Pending,
            Err(err) => return Poll::Ready(Err(PipeError::Io(err))),
        }
    };
}

impl<A: Adapter> Future for Pipe<A>
where
    <A as Adapter>::Output: Send,
    <A as Adapter>::Error: Send,
{
    type Output = Result<A::Output, PipeError<A::Error>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        use crate::adapter::AdapterAction;

        let mut buf = [0; 1024];
        let n_read = poll_future_pipe!(self.r.poll_read(&mut buf));

        let mut view = BytesMut::new();
        view.extend_from_slice(&buf[..n_read]);

        let adapter_result = match &self.t {
            PipeType::Incoming => ready!(self.a.poll_handle_incoming(cx, view)),
            PipeType::Outgoing => ready!(self.a.poll_handle_outgoing(cx, view)),
        };

        match adapter_result {
            Ok(res) => match res {
                AdapterAction::WriteAndEnd(val) => {
                    poll_future_pipe!(self.w.poll_write(&buf[0..n_read]));
                    Poll::Ready(Ok(val))
                },
                AdapterAction::Write => {
                    poll_future_pipe!(self.w.poll_write(&buf[0..n_read]));
                    Poll::Pending
                },
                AdapterAction::End(val) => Poll::Ready(Ok(val))
            },
            Err(err) => Poll::Ready(Err(PipeError::Custom(err))),
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

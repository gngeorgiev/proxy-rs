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

use crate::builder::ProxyBuilder;
use crate::socket::Socket;
use crate::poll_future_01_in_03;
use crate::util::tracer;

use smallvec::SmallVec;
use std::time::Duration;

pub type ProxyEvent = Result<(), ((), io::Error)>;
pub type ProxyEventReceiver = UnboundedReceiver<ProxyEvent>;
type ProxyEventSender = UnboundedSender<ProxyEvent>;

pub struct Proxy {
    pub(crate) bind_addr: SocketAddr,
    pub(crate) remote_addr: SocketAddr,
    pub(crate) pool: Pool<TcpConnection>,
}

impl Proxy {
    pub fn builder() -> ProxyBuilder {
        ProxyBuilder::new()
    }

    pub fn bind(&self, executor: TaskExecutor) -> io::Result<ProxyEventReceiver> {
        let remote_addr = self.remote_addr;
        let pool = self.pool.clone();

        let listener = TcpListener::bind(&self.bind_addr)?;
        let server = listener.incoming();

        let (s, r) = unbounded();
        let incoming_future = handle_incoming(executor.clone(), server, s, pool);
        executor.spawn(
            incoming_future
                .unit_error()
                .boxed()
                .compat()
        );

        Ok(r)
    }
}

async fn handle_incoming(mut executor: TaskExecutor, server: Incoming, event_sender: ProxyEventSender, pool: Pool<TcpConnection>) {
    let mut server = server.compat();

//    pool.initialize(150).await.unwrap();

    while let Some(raw_incoming_socket) = server.next().await {
        let raw_incoming_socket: TcpStream = raw_incoming_socket.unwrap();
        raw_incoming_socket.set_nodelay(true).unwrap();
        raw_incoming_socket.set_linger(None).unwrap();
        raw_incoming_socket.set_keepalive(Some(Duration::from_secs(30))).unwrap();

        let mut s = event_sender.clone();
        let incoming_socket = Socket::new(raw_incoming_socket);
        let pool = pool.clone();

        let socket_future = async move {
            let res = socket_handler(incoming_socket.clone(), pool).await;
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

async fn socket_handler(
    incoming_socket: Socket,
    pool: Pool<TcpConnection>,
) -> io::Result<()> {
    let mut local_socket = pool.take().await?;

    let local_socket = Socket::new(local_socket.detach().unwrap().0);

    let (local_read, local_write) = local_socket.clone().split();
    let (incoming_read, incoming_write) = incoming_socket.split();

//    let incoming_to_local = pipe(incoming_read, local_write, Parser::request());
//    let local_to_incoming = pipe(local_read, incoming_write, Parser::response());

//    local_socket.bytes()

    let incoming_to_local = Pipe::new(incoming_read, local_write, Parser::request());
    let local_to_incoming = Pipe::new(local_read, incoming_write, Parser::response());

    let a = async move {
//        let _guard = tracer::new("incoming_to_local").at_least_duration(Duration::from_millis(1));
//        let _guard = crate::util::Elapsed::new("incoming_to_local");
        incoming_to_local.await.unwrap();
    };

    let b = async move {
        let mut _guard = tracer::new("local_to_incoming").at_least_duration(Duration::from_millis(1));
//        let _guard = crate::util::Elapsed::new("local_to_incoming");
        local_to_incoming.await.unwrap();
        drop(_guard);
    };

    join!(a, b);
//    try_join!(incoming_to_local, local_to_incoming)?;


    let tcp_connection = TcpConnection::new(local_socket.raw());
    pool.put(tcp_connection);

    Ok(())
}

struct Pipe {
    r: ReadHalf<Socket>,
    w: WriteHalf<Socket>,
    p: Parser,
    t: crate::util::tracer::Tracer,
}

impl Pipe {
    fn new(r: ReadHalf<Socket>, w: WriteHalf<Socket>, p: Parser) -> Self {
        let t = crate::util::tracer::new("pipe")
                .at_least_duration(Duration::from_millis(1));
        Pipe {r, w, p, t}
    }
}

impl Future for Pipe {
    type Output = io::Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut buf = [0; 1024];
        let trace_read = self.t.trace("read");
        let n_read = poll_future_01_in_03!(self.r.poll_read(&mut buf));
        self.t.done(trace_read);

        let trace_write = self.t.trace("write");
        let n_write = poll_future_01_in_03!(self.w.poll_write(&buf[0..n_read]));
        self.t.done(trace_write);

        let mut h = Default::default();
        let trace_parse = self.t.trace("parse");
        self.p.parse::<HttpParserHandler>(&mut h, &buf);
        self.t.done(trace_parse);

        if h.done {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
}

async fn pipe(from: Socket, to: Socket, mut parser: Parser) {
//    let mut p: HttpParserHandler = Default::default();

//    let (mut read, _) = from.split();
//    let (_, write) = to.split();
//
//    let mut buf = vec![];
//    read.poll_read(&mut buf).compat().await;


//    let from = Framed::new(from, BytesCodec::new());
//    let to = Framed::new(to, BytesCodec::new());

//    let mut from = from.compat();
//    let mut to = to.sink_compat();

//    while let Some(Ok(mut data)) = from.next().await {
//        let buffer = data.take().freeze();
//        parser.parse(&mut p, &buffer[..]);
//        let _guard = crate::util::Elapsed::new("pipe");
//        to.send(buffer).await.unwrap();
//        if p.done {
//            break;
//        }
//    }
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

use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use futures::compat::*;
use futures::task::Context;
use futures::channel::mpsc::{unbounded, UnboundedSender, UnboundedReceiver};
use futures::{
    future, join, ready, Future, FutureExt, Poll, SinkExt, Stream, StreamExt, TryFutureExt,
};

use tokio::codec::{BytesCodec, Framed};
use tokio::io::{AsyncRead, AsyncWrite, Read, Write};
use tokio::net::tcp::Incoming;
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::TaskExecutor;

use http_muncher::{Parser, ParserHandler};

use fut_pool::Pool;
use fut_pool_tcp::TcpConnection;

pub struct Proxy {
    pub(crate) bind_addr: SocketAddr,
    pub(crate) remote_addr: SocketAddr,
    pub(crate) pool: Pool<TcpConnection>,
}

impl Proxy {
    pub async fn bind(mut self: Pin<&mut Self>, executor: TaskExecutor) -> io::Result<UnboundedReceiver<Result<TcpStream, (TcpStream, io::Error)>>> {
        let remote_addr = self.remote_addr;
        let pool = self.pool.clone();

        let listener = TcpListener::bind(&self.bind_addr)?;
        let mut server = listener.incoming().compat();

        let (s, r) = unbounded();
        while let Some(raw_incoming_socket) = server.next().await {
            let mut s = s.clone();
            let incoming_socket = Socket::new(raw_incoming_socket.unwrap());
            let pool = pool.clone();

            let socket_future = async move {
                let res = socket_handler(incoming_socket.clone(), pool).await;
                let raw_incoming_socket = incoming_socket.raw();
                match res {
                    Ok(_) => s.send(Ok(raw_incoming_socket)),
                    Err(err) => s.send(Err((raw_incoming_socket, err)))
                }.await.expect("sending request/response result to channel");
            };

            executor.spawn(
                socket_future
                    .unit_error()
                    .boxed()
                    .compat()
            );
        }

        Ok(r)
    }
}

async fn socket_handler(
    incoming_socket: Socket,
    pool: Pool<TcpConnection>,
) -> io::Result<()> {
    let mut local_socket = pool.take().await?;

    let local_socket = Socket::new(local_socket.detach().unwrap().0);
    let local_read = local_socket.clone();
    let local_write = local_socket.clone();

    let incoming_read = incoming_socket.clone();
    let incoming_write = incoming_socket.clone();

    let incoming_to_local = pipe(incoming_read, local_write, Parser::request());
    let local_to_incoming = pipe(local_read, incoming_write, Parser::response());
    join!(incoming_to_local, local_to_incoming);

    let raw_local_socket = local_socket.raw();
    let tcp_connection = TcpConnection(raw_local_socket);
    pool.put(tcp_connection);

    Ok(())
}

async fn pipe(from: Socket, to: Socket, mut parser: Parser) {
    let mut p: HttpParserHandler = Default::default();

    let from = Framed::new(from, BytesCodec::new());
    let to = Framed::new(to, BytesCodec::new());

    let mut from = from.compat();
    let mut to = to.sink_compat();
    while let Some(Ok(mut data)) = from.next().await {
        let buffer = data.take().freeze();
        parser.parse(&mut p, &buffer[..]);
        to.send(buffer).await.unwrap();
        if p.done {
            break;
        }
    }
}

#[derive(Default)]
struct HttpParserHandler {
    done: bool,
}

impl ParserHandler for HttpParserHandler {
    fn on_message_complete(&mut self, _parser: &mut Parser) -> bool {
        self.done = true;
        true
    }
}

#[derive(Clone)]
struct Socket(Arc<TcpStream>);

impl Socket {
    fn new(raw_socket: TcpStream) -> Self {
        Socket(Arc::new(raw_socket))
    }

    fn raw(self) -> TcpStream {
        Arc::try_unwrap(self.0).expect("getting raw TcpStream")
    }
}

impl Read for Socket {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&*self.0).read(buf)
    }
}

impl Write for Socket {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&*self.0).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl AsyncRead for Socket {}

impl AsyncWrite for Socket {
    fn shutdown(&mut self) -> tokio::prelude::Poll<(), io::Error> {
        Ok(().into())
    }
}

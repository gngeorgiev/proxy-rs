#![feature(async_await)]

use std::sync::Arc;

use fut_pool::*;
use fut_pool_tcp::TcpConnection;

use failure::Error;

use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::TaskExecutor;
use tokio::io;
use tokio::prelude::*;
use tokio::codec::{Framed, BytesCodec};

use futures::compat::*;
use futures::{StreamExt, TryFutureExt, FutureExt, SinkExt};
use futures::{join};

use http_muncher::{Parser, ParserHandler};

fn main() {
    pretty_env_logger::init();

    let addr = "127.0.0.1:8080".parse().unwrap();
    let pool = Pool::builder()
        .factory(move || {
            // dbg!("connect");
            TcpConnection::connect(addr)
        })
        .build();

    let mut rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(server(rt.executor(), pool).map_err(|err| {
        eprintln!("server error {}", err);
    }).boxed().compat()).unwrap();
}

async fn server(executor: TaskExecutor, pool: Pool<TcpConnection>) -> Result<(), Error> {
    let bind_addr = "127.0.0.1:8081".parse()?;
    let mut server = TcpListener::bind(&bind_addr)?.incoming().compat();

    while let Some(socket) = server.next().await {
        let socket = socket.unwrap();
        let pool = pool.clone();
        executor.spawn(socket_handler(socket, pool).map_err(|err| {
            eprintln!("socket err {}", err);
        }).boxed().compat());
    }

    // dbg!("done server");
    Ok(())
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
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        Ok(().into())
    }
}

async fn socket_handler(incoming_socket: TcpStream, pool: Pool<TcpConnection>) -> Result<(), Error> {
    // dbg!("socket handler");
    // dbg!(pool.size());
    let mut local_socket = pool.take().await?;
    // dbg!(pool.size());
    let local_socket = local_socket.detach().unwrap();

    let incoming_socket = Socket(Arc::new(incoming_socket));
    let incoming_read = incoming_socket.clone();
    let incoming_write = incoming_socket.clone();

    let local_socket = Socket(Arc::new(local_socket.0));
    let local_read = local_socket.clone();
    let local_write = local_socket.clone();

//    let (incoming_write, incoming_read) = Framed::new(incoming_socket, BytesCodec::new()).split();
//    let (local_write, local_read) = Framed::new(local_socket.0, BytesCodec::new()).split();

    let incoming_to_local = pipe(incoming_read, local_write, Parser::request());
    let local_to_incoming = pipe(local_read, incoming_write, Parser::response());
    join!(incoming_to_local, local_to_incoming);

    let raw_socket = Arc::try_unwrap(local_socket.0).unwrap();
    let tcp_connection = TcpConnection(raw_socket);
    pool.put(tcp_connection);

    // dbg!("done socket");

    Ok(())
}

async fn pipe(from: Socket, to: Socket, mut parser: Parser) {
    // dbg!(annotation);
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
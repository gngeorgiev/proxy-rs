#![feature(async_await)]

use std::str;

use fut_pool::*;
use fut_pool_tcp::TcpConnection;

use failure::Error;

use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::TaskExecutor;
use tokio::io;
use tokio::prelude::*;
use tokio::codec::{Framed, BytesCodec};
use tokio::prelude::stream::{SplitSink, SplitStream};

use futures::compat::*;
use futures::{StreamExt, TryFutureExt, FutureExt, SinkExt};
use futures::channel::mpsc::{channel, Sender};
use futures::{join};

use http_muncher::{Parser, ParserHandler};

fn main() {
    let addr = "127.0.0.1:8080".parse().unwrap();
    let pool = Pool::builder()
        .factory(move || {
            dbg!("connect");
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
        executor.spawn(socket_handler(executor.clone(), socket, pool).map_err(|err| {
            eprintln!("socket err {}", err);
        }).boxed().compat());
    }

    dbg!("done server");
    Ok(())
}

#[derive(Default)]
struct HttpParserHandler {
    done: bool,
}

impl ParserHandler for HttpParserHandler {
    fn on_message_complete(&mut self, parser: &mut Parser) -> bool {
        self.done = true;
        true
    }
}

async fn socket_handler(executor: TaskExecutor, incoming_socket: TcpStream, pool: Pool<TcpConnection>) -> Result<(), Error> {
    dbg!("socket handler");
    dbg!(pool.size());
    let mut local_socket = pool.take().await?;
    dbg!(pool.size());
    let local_socket = local_socket.detach().unwrap();

    let (incoming_write, incoming_read) = Framed::new(incoming_socket, BytesCodec::new()).split();
    let (local_write, local_read) = Framed::new(local_socket.0, BytesCodec::new()).split();

    let incoming_to_local = pipe("incoming",&incoming_read, &local_write, Parser::request());
    let local_to_incoming = pipe("local", &local_read, &incoming_write, Parser::response());
    join!(incoming_to_local, local_to_incoming);

    let local_socket = local_write.reunite(local_read);

    dbg!("done socket");

    Ok(())
}

async fn pipe<'a>(annotation: &'static str, mut from: &'a SplitStream<Framed<TcpStream, BytesCodec>>, mut to: &'a SplitSink<Framed<TcpStream, BytesCodec>>, mut parser: Parser) {
    let mut p: HttpParserHandler = Default::default();

    let mut from = from.clone().compat();
    let mut to = to.sink_compat();
    while let Some(Ok(mut data)) = from.next().await {
        let buffer = data.take().freeze();
        parser.parse(&mut p, &buffer[..]);
        to.send(buffer).await.unwrap();
        if p.done {
            break;
        }
    }

    dbg!(format!("{} done", annotation));

    unimplemented!()
//    Ok((from, to))
}
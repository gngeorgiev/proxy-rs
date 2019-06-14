#![feature(async_await)]

#[macro_use]
extern crate log;

use std::sync::Arc;

use fut_pool::*;
use fut_pool_tcp::TcpConnection;

use failure::Error;

use tokio::codec::{BytesCodec, Framed};
use tokio::io;
use tokio::net::{TcpListener, TcpStream};
use tokio::prelude::*;
use tokio::runtime::TaskExecutor;

use futures::compat::*;
use futures::join;
use futures::{FutureExt, SinkExt, StreamExt, TryFutureExt};

use http_muncher::{Parser, ParserHandler};

use proxy_rs::{Proxy, ProxyEventReceiver};

fn main() -> Result<(), Error> {
    pretty_env_logger::init();

    let proxy = Proxy::builder()
        .bind_addr("127.0.0.1:8081".parse()?)
        .remote_addr("127.0.0.1:1338".parse()?)
        .build();

    let mut rt = tokio::runtime::Runtime::new().unwrap();

    let ev = proxy.bind(rt.executor())?;
    rt.block_on(
        handle_proxy_events(ev)
            .unit_error()
            .boxed()
            .compat()
    ).unwrap();

    Ok(())
}

async fn handle_proxy_events(mut receiver: ProxyEventReceiver) {
    while let Some(ev) = receiver.next().await {
        match ev {
            Ok(socket) => debug!("processed request for socket {:?}", socket),
            Err((socket, err)) => error!("error {} for socket {:?}", err, socket)
        }
    }
}

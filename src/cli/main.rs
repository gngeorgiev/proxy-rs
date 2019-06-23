#![feature(async_await)]
#![allow(warnings)]

#[macro_use]
extern crate log;

use std::sync::Arc;

use fut_pool::*;
use fut_pool::tcp::TcpConnection;

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

use proxy_rs::{Proxy, ProxyEventReceiver, adapter::http::HttpAdapter};
use chrono::Timelike;
use std::sync::atomic::{AtomicUsize, Ordering};

fn main() -> Result<(), Error> {
    pretty_env_logger::init_timed();

    let proxy = Proxy::builder()
        .bind_addr("127.0.0.1:8081".parse()?)
        .remote_addr("127.0.0.1:1338".parse()?)
        .adapter(|| HttpAdapter::new())
        .build();

    let mut rt = tokio::runtime::Builder::new()
        .core_threads(num_cpus::get())
        .build()
        .unwrap();

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
//        match ev {
//            Ok(socket) => debug!("processed request for socket {:?}", socket),
//            Err((socket, err)) => error!("error {} for socket {:?}", err, socket)
//        }
    }
}

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

use proxy_rs::{Proxy, ProxyEventReceiver, adapter::http::HttpAdapter, ProxyHttp, ProxyEventReceiverHttp};
use chrono::Timelike;
use std::sync::atomic::{AtomicUsize, Ordering};
use proxy_rs::adapter::Adapter;
use proxy_rs::adapter::passthrough::PassthroughAdapter;

fn main() -> Result<(), Error> {
    pretty_env_logger::init_timed();

    let mut rt = tokio::runtime::Builder::new()
        .core_threads(num_cpus::get())
        .build()
        .unwrap();

    let mut proxy = ProxyHttp::builder()
        .bind_addr("127.0.0.1:8081".parse()?)
        .remote_addr("127.0.0.1:1338".parse()?)
        .build();

    let events = proxy.accept_events();

    proxy.listen(rt.executor())?;

    rt.block_on(
    handle_proxy_events(events)
            .unit_error()
            .boxed()
            .compat()
    ).expect("handle proxy events");

    Ok(())
}

async fn handle_proxy_events(mut receiver: ProxyEventReceiverHttp) {
    while let Some(ev) = receiver.next().await {
        match ev {
            Ok(ev) => debug!("processed request/response {:?}", ev),
            Err(err) => error!("error {:?}", err)
        }
    }
}

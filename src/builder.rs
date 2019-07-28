use fut_pool::tcp::TcpConnection;
use fut_pool::Pool;

use std::fmt::Debug;
use std::io::Result;
use std::net::SocketAddr;
use std::pin::Pin;

use tokio::net::TcpStream;

use futures::compat::*;

use crate::adapter::http::HttpAdapter;
use crate::adapter::Adapter;

use crate::proxy::Proxy;
use crate::socket::Socket;
use std::marker::PhantomData;
use std::sync::Arc;

pub struct ProxyBuilder<A: Adapter> {
    _bind_addr: Option<SocketAddr>,
    _remote_addr: Option<SocketAddr>,
    _pool: Option<Pool<TcpConnection>>,
    _adapter: PhantomData<A>,
}

impl<A: Adapter> ProxyBuilder<A> {
    pub(crate) fn new() -> Self {
        ProxyBuilder {
            _bind_addr: None,
            _remote_addr: None,
            _pool: None,
            _adapter: Default::default(),
        }
    }

    pub fn bind_addr(mut self, bind_addr: SocketAddr) -> Self {
        self._bind_addr = Some(bind_addr);
        self
    }

    pub fn remote_addr(mut self, remote_addr: SocketAddr) -> Self {
        self._remote_addr = Some(remote_addr);
        self
    }

    pub fn with_pool(mut self, pool: Pool<TcpConnection>) -> Self {
        self._pool = Some(pool);
        self
    }

    pub fn build(self) -> Proxy<A> {
        let bind_addr = self._bind_addr.expect("bind_addr is required");
        let remote_addr = self._remote_addr.expect("remote_addr is required");
        let proxy = Proxy {
            bind_addr,
            remote_addr,
            events: None,
            pool: self._pool.clone().unwrap_or_else(move || {
                Pool::builder()
                    .factory(move || {
                        debug!("creating new TcpConnection for pool");
                        TcpConnection::connect(remote_addr)
                    })
                    .capacity(Some(400))
                    .build()
            }),
        };

        proxy
    }
}

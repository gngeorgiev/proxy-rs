use std::io;

use crate::adapter::AdapterAction::WriteAndEnd;
use bytes::{Bytes, BytesMut};
use futures::task::Context;
use futures::{ready, try_ready, AsyncRead, Future, Poll};
use std::pin::Pin;

pub mod http;

pub enum AdapterAction<T> {
    Write,
    WriteAndEnd(T),
    End(T),
}

pub trait Adapter: Send + Sync + Unpin + Default {
    type Output;
    type Error;

    fn poll_handle_incoming(
        &mut self,
        cx: &mut Context,
        mut incoming: BytesMut,
    ) -> Poll<Result<AdapterAction<Self::Output>, Self::Error>>;

    fn poll_handle_outgoing(
        &mut self,
        cx: &mut Context,
        mut outgoing: BytesMut,
    ) -> Poll<Result<AdapterAction<Self::Output>, Self::Error>>;
}

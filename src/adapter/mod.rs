use std::io;

use crate::adapter::AdapterAction::WriteAndEnd;
use bytes::{Bytes, BytesMut};
use futures::task::Context;
use futures::{ready, try_ready, AsyncRead, Future, Poll};
use std::pin::Pin;

pub mod http;

pub enum AdapterAction {
    Write,
    WriteAndEnd,
    End,
}

pub trait Adapter: Send + Sync + Unpin {
    fn poll_handle_incoming(
        &mut self,
        cx: &mut Context<'_>,
        mut incoming: BytesMut,
    ) -> Poll<io::Result<AdapterAction>> {
        Poll::Ready(Ok(WriteAndEnd))
    }

    fn poll_handle_outgoing(
        &mut self,
        cx: &mut Context<'_>,
        mut outgoing: BytesMut,
    ) -> Poll<io::Result<AdapterAction>> {
        Poll::Ready(Ok(WriteAndEnd))
    }
}

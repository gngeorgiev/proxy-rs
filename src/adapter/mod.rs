use std::io;

use crate::adapter::AdapterAction::WriteAndEnd;
use bytes::{Bytes, BytesMut};
use futures::task::Context;
use futures::{ready, try_ready, AsyncRead, Future, Poll};
use std::fmt::Debug;
use std::pin::Pin;

pub mod http;
pub mod passthrough;

#[derive(Debug)]
pub enum AdapterAction<T: Debug> {
    Write,
    WriteAndEnd(T),
    End(T),
}

pub trait Adapter: Send + Sync + Unpin + Default + Debug
where
    <Self as Adapter>::IncomingOutput: Send + Sync + Debug + Unpin,
    <Self as Adapter>::OutgoingOutput: Send + Sync + Debug + Unpin,
    <Self as Adapter>::Error: Send + Sync + Debug + Unpin,
{
    type IncomingOutput;
    type OutgoingOutput;
    type Error;

    fn buffer_size(&self) -> usize {
        1024
    }

    fn poll_handle_incoming(
        &mut self,
        cx: &mut Context,
        incoming: &mut BytesMut,
    ) -> Poll<Result<AdapterAction<Self::IncomingOutput>, Self::Error>> {
        Poll::Ready(Ok(AdapterAction::Write))
    }

    fn poll_handle_outgoing(
        &mut self,
        cx: &mut Context,
        mut outgoing: &mut BytesMut,
    ) -> Poll<Result<AdapterAction<Self::OutgoingOutput>, Self::Error>> {
        Poll::Ready(Ok(AdapterAction::Write))
    }
}

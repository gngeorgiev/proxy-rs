use crate::adapter::{Adapter, AdapterAction};
use bytes::BytesMut;
use futures::task::Context;
use futures::Poll;

#[derive(Default, Debug)]
pub struct PassthroughAdapter;

impl Adapter for PassthroughAdapter {
    type IncomingOutput = ();
    type OutgoingOutput = ();
    type Error = ();
}

use bytes::{Bytes, BytesMut};
use futures::{try_ready, AsyncRead, Poll};
use std::io;

use crate::adapter::{Adapter, AdapterAction};
use futures::task::Context;
use http_muncher::{Parser, ParserHandler};
use parking_lot::Mutex;
use std::cell::RefCell;
use std::pin::Pin;

#[derive(Default)]
struct HttpParserHandler {
    done: bool,
}

impl ParserHandler for HttpParserHandler {
    #[inline]
    fn on_message_complete(&mut self, _parser: &mut Parser) -> bool {
        self.done = true;
        true
    }
}

#[derive(Default)]
pub struct HttpAdapter {
    parser: Option<Mutex<Parser>>,
    handler: Option<Mutex<HttpParserHandler>>,
}

impl std::fmt::Debug for HttpAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

impl Adapter for HttpAdapter {
    type IncomingOutput = ();
    type OutgoingOutput = ();
    type Error = ();

    fn poll_handle_incoming(
        &mut self,
        cx: &mut Context<'_>,
        incoming: &mut BytesMut,
    ) -> Poll<Result<AdapterAction<Self::IncomingOutput>, Self::Error>> {
        let parser = self
            .parser
            .get_or_insert_with(|| Mutex::new(Parser::request()));
        let handler = self
            .handler
            .get_or_insert_with(|| Mutex::new(HttpParserHandler::default()));

        parser
            .get_mut()
            .parse::<HttpParserHandler>(handler.get_mut(), &incoming[..]);

        Poll::Ready(Ok(match handler.get_mut().done {
            true => AdapterAction::WriteAndEnd(()),
            false => AdapterAction::Write,
        }))
    }

    fn poll_handle_outgoing(
        &mut self,
        cx: &mut Context<'_>,
        outgoing: &mut BytesMut,
    ) -> Poll<Result<AdapterAction<Self::OutgoingOutput>, Self::Error>> {
        let parser = self
            .parser
            .get_or_insert_with(|| Mutex::new(Parser::response()));
        let handler = self
            .handler
            .get_or_insert_with(|| Mutex::new(HttpParserHandler::default()));

        parser
            .get_mut()
            .parse::<HttpParserHandler>(handler.get_mut(), &outgoing[..]);

        Poll::Ready(Ok(match handler.get_mut().done {
            true => AdapterAction::WriteAndEnd(()),
            false => AdapterAction::Write,
        }))
    }
}

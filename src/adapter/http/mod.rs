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

pub struct HttpAdapter {
    parser: Option<Mutex<Parser>>,
    handler: Option<Mutex<HttpParserHandler>>,
}

impl HttpAdapter {
    pub fn new() -> Self {
        HttpAdapter {
            parser: None,
            handler: None,
        }
    }
}

impl Adapter for HttpAdapter {
    fn poll_handle_incoming(
        &mut self,
        cx: &mut Context<'_>,
        incoming: BytesMut,
    ) -> Poll<io::Result<AdapterAction>> {
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
            true => AdapterAction::WriteAndEnd,
            false => AdapterAction::Write,
        }))
    }

    fn poll_handle_outgoing(
        &mut self,
        cx: &mut Context<'_>,
        outgoing: BytesMut,
    ) -> Poll<io::Result<AdapterAction>> {
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
            true => AdapterAction::WriteAndEnd,
            false => AdapterAction::Write,
        }))
    }
}

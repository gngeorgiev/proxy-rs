use std::sync::Arc;

use tokio::io::{self, AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::prelude::{Read, Write};

#[derive(Clone)]
pub struct Socket(pub Arc<TcpStream>);

impl Socket {
    pub fn new(raw_socket: TcpStream) -> Self {
        Socket(Arc::new(raw_socket))
    }

    pub fn raw(self) -> TcpStream {
        Arc::try_unwrap(self.0).expect("getting raw TcpStream")
    }
}

impl Read for Socket {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&*self.0).read(buf)
    }
}

impl Write for Socket {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&*self.0).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl AsyncRead for Socket {}

impl AsyncWrite for Socket {
    fn shutdown(&mut self) -> tokio::prelude::Poll<(), io::Error> {
        Ok(().into())
    }
}

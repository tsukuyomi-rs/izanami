#![cfg(feature = "rustls")]

use {
    super::*,
    ::rustls::{ServerConfig, ServerSession, Session, Stream},
    futures::Poll,
    std::{io, sync::Arc},
};

#[allow(missing_debug_implementations)]
#[derive(Clone)]
pub struct TlsAcceptor {
    config: Arc<ServerConfig>,
}

impl From<ServerConfig> for TlsAcceptor {
    fn from(config: ServerConfig) -> Self {
        Self::from(Arc::new(config))
    }
}

impl From<Arc<ServerConfig>> for TlsAcceptor {
    fn from(config: Arc<ServerConfig>) -> Self {
        Self { config }
    }
}

impl<T> Acceptor<T> for TlsAcceptor
where
    T: AsyncRead + AsyncWrite,
{
    type Accepted = TlsStream<T>;

    #[inline]
    fn accept(&self, io: T) -> Self::Accepted {
        TlsStream {
            io,
            is_shutdown: false,
            session: ServerSession::new(&self.config),
        }
    }

    fn is_tls(&self) -> bool {
        true
    }
}

#[derive(Debug)]
pub struct TlsStream<S> {
    io: S,
    is_shutdown: bool,
    session: ServerSession,
}

impl<S> TlsStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    #[inline]
    fn stream(&mut self) -> Stream<'_, ServerSession, S> {
        Stream::new(&mut self.session, &mut self.io)
    }
}

impl<S> io::Read for TlsStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream().read(buf)
    }
}

impl<S> io::Write for TlsStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream().flush()?;
        self.io.flush()
    }
}

impl<S> AsyncRead for TlsStream<S> where S: AsyncRead + AsyncWrite {}

impl<S> AsyncWrite for TlsStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        if self.session.is_handshaking() {
            return Ok(().into());
        }

        if !self.is_shutdown {
            self.session.send_close_notify();
            self.is_shutdown = true;
        }

        tokio_io::try_nb!(io::Write::flush(self));
        self.io.shutdown()
    }
}

#![cfg(feature = "rustls")]

use {
    super::*,
    ::rustls::{ServerConfig, ServerSession, Session, Stream},
    futures::Poll,
    std::{io, sync::Arc},
};

impl<T> TlsConfig<T> for Arc<ServerConfig>
where
    T: AsyncRead + AsyncWrite,
{
    type Wrapped = RustlsStream<T>;
    type Wrapper = Self;

    #[inline]
    fn into_wrapper(self, _: Vec<String>) -> crate::Result<Self::Wrapper> {
        Ok(self)
    }
}

impl<T> TlsConfig<T> for ServerConfig
where
    T: AsyncRead + AsyncWrite,
{
    type Wrapped = RustlsStream<T>;
    type Wrapper = Arc<ServerConfig>;

    #[inline]
    fn into_wrapper(mut self, alpn_protocols: Vec<String>) -> crate::Result<Self::Wrapper> {
        self.set_protocols(&alpn_protocols);
        Ok(Arc::new(self))
    }
}

impl<T> TlsWrapper<T> for Arc<ServerConfig>
where
    T: AsyncRead + AsyncWrite,
{
    type Wrapped = RustlsStream<T>;

    #[inline]
    fn wrap(&self, io: T) -> Self::Wrapped {
        RustlsStream {
            io,
            is_shutdown: false,
            session: ServerSession::new(self),
        }
    }
}

#[derive(Debug)]
pub struct RustlsStream<S> {
    io: S,
    is_shutdown: bool,
    session: ServerSession,
}

impl<S> RustlsStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    #[inline]
    fn stream(&mut self) -> Stream<'_, ServerSession, S> {
        Stream::new(&mut self.session, &mut self.io)
    }
}

impl<S> io::Read for RustlsStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream().read(buf)
    }
}

impl<S> io::Write for RustlsStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream().write(buf)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.stream().flush()?;
        self.io.flush()
    }
}

impl<S> AsyncRead for RustlsStream<S> where S: AsyncRead + AsyncWrite {}

impl<S> AsyncWrite for RustlsStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    #[inline]
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

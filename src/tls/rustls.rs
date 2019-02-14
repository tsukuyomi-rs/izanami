#![cfg(feature = "use-rustls")]

use {
    super::*,
    ::rustls::{ServerConfig, ServerSession},
    std::{io, sync::Arc},
    tokio_rustls::{Accept, TlsAcceptor, TlsStream},
};

impl<T> TlsConfig<T> for Arc<ServerConfig>
where
    T: AsyncRead + AsyncWrite,
{
    type Wrapped = TlsStream<T, ServerSession>;
    type Wrapper = TlsAcceptor;

    #[inline]
    fn into_wrapper(self, _: Vec<String>) -> crate::Result<Self::Wrapper> {
        Ok(TlsAcceptor::from(self))
    }
}

impl<T> TlsConfig<T> for ServerConfig
where
    T: AsyncRead + AsyncWrite,
{
    type Wrapped = TlsStream<T, ServerSession>;
    type Wrapper = TlsAcceptor;

    #[inline]
    fn into_wrapper(mut self, alpn_protocols: Vec<String>) -> crate::Result<Self::Wrapper> {
        self.set_protocols(&alpn_protocols);
        Ok(TlsAcceptor::from(Arc::new(self)))
    }
}

impl<T> TlsWrapper<T> for TlsAcceptor
where
    T: AsyncRead + AsyncWrite,
{
    type Wrapped = TlsStream<T, ServerSession>;
    type Error = io::Error;
    type Future = Accept<T>;

    #[inline]
    fn wrap(&self, io: T) -> Self::Future {
        self.accept(io)
    }
}

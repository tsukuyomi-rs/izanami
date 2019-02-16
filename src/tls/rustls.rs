#![cfg(feature = "use-rustls")]

use {
    super::*,
    crate::util::MapAsyncExt,
    ::rustls::{ServerConfig, ServerSession},
    futures::Poll,
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
    type Future = AcceptWithSni<T>;

    #[inline]
    fn wrap(&self, io: T) -> Self::Future {
        AcceptWithSni {
            inner: self.accept(io),
        }
    }
}

#[allow(missing_debug_implementations)]
pub struct AcceptWithSni<T> {
    inner: Accept<T>,
}

impl<T> Future for AcceptWithSni<T>
where
    T: AsyncRead + AsyncWrite,
{
    type Item = (TlsStream<T, ServerSession>, Option<ServerName>);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.inner.poll().map_async(|conn| {
            let sni_hostname = conn
                .get_ref()
                .1
                .get_sni_hostname()
                .map(|sni| ServerName::Dns(sni.into()));
            (conn, sni_hostname)
        })
    }
}

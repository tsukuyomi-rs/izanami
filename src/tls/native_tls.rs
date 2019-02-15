#![cfg(feature = "use-native-tls")]

use {
    super::*,
    crate::util::MapAsyncExt,
    tokio_tls::{Accept, TlsAcceptor, TlsStream},
};

impl<T> TlsConfig<T> for ::native_tls::TlsAcceptor
where
    T: AsyncRead + AsyncWrite,
{
    type Wrapped = TlsStream<T>;
    type Wrapper = TlsAcceptor;

    #[inline]
    fn into_wrapper(self, _: Vec<String>) -> crate::Result<Self::Wrapper> {
        Ok(self.into())
    }
}

impl<T> TlsWrapper<T> for TlsAcceptor
where
    T: AsyncRead + AsyncWrite,
{
    type Wrapped = TlsStream<T>;
    type Error = ::native_tls::Error;
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
    type Item = (TlsStream<T>, SniHostname);
    type Error = ::native_tls::Error;

    #[inline]
    fn poll(&mut self) -> futures::Poll<Self::Item, Self::Error> {
        self.inner
            .poll()
            .map_async(|conn| (conn, SniHostname::unknown()))
    }
}

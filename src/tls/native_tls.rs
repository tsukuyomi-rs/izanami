#![cfg(feature = "use-native-tls")]

use {
    super::*,
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
    type Future = Accept<T>;

    #[inline]
    fn wrap(&self, io: T) -> Self::Future {
        self.accept(io)
    }
}

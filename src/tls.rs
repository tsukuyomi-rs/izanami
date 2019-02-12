//! SSL/TLS support.

#[path = "tls/native_tls.rs"]
pub mod native_tls;
pub mod openssl;
pub mod rustls;

use tokio::io::{AsyncRead, AsyncWrite};

/// Trait representing a SSL/TLS configuration.
pub trait TlsConfig<T> {
    type Wrapped: AsyncRead + AsyncWrite;
    type Wrapper: TlsWrapper<T, Wrapped = Self::Wrapped>;

    /// Creates an instance of `Acceptor` using the specified configuration.
    fn into_wrapper(self, alpn_protocols: Vec<String>) -> crate::Result<Self::Wrapper>;
}

/// Trait representing a converter for granting the SSL/TLS to asynchronous I/Os.
pub trait TlsWrapper<T>: Clone {
    /// The type of wrapped asynchronous I/O returned from `accept`.
    type Wrapped: AsyncRead + AsyncWrite;

    /// Wraps the specified I/O object in the SSL/TLS stream.
    fn wrap(&self, io: T) -> Self::Wrapped;
}

/// The default `TlsConfig` that returns the input I/O directly.
#[derive(Debug, Default, Clone)]
pub struct NoTls(());

impl<T> TlsConfig<T> for NoTls
where
    T: AsyncRead + AsyncWrite,
{
    type Wrapped = T;
    type Wrapper = Self;

    #[inline]
    fn into_wrapper(self, _: Vec<String>) -> crate::Result<Self::Wrapper> {
        Ok(self.clone())
    }
}

impl<T> TlsWrapper<T> for NoTls
where
    T: AsyncRead + AsyncWrite,
{
    type Wrapped = T;

    #[inline]
    fn wrap(&self, io: T) -> Self::Wrapped {
        io
    }
}

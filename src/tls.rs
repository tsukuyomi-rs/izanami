//! SSL/TLS support.

#[path = "tls/native_tls.rs"]
pub mod native_tls;
pub mod openssl;
pub mod rustls;

use tokio::io::{AsyncRead, AsyncWrite};

/// A set of values passed to `Tls::wrapper`.
#[derive(Debug)]
pub struct TlsConfig {
    pub(crate) alpn_protocols: Vec<String>,
}

/// Trait representing a SSL/TLS configuration.
pub trait Tls<T> {
    type Wrapped: AsyncRead + AsyncWrite;
    type Wrapper: TlsWrapper<T, Wrapped = Self::Wrapped>;

    /// Creates an instance of `Acceptor` using the specified configuration.
    fn wrapper(&self, config: TlsConfig) -> crate::Result<Self::Wrapper>;
}

/// Trait representing a converter for granting the SSL/TLS to asynchronous I/Os.
pub trait TlsWrapper<T> {
    /// The type of wrapped asynchronous I/O returned from `accept`.
    type Wrapped: AsyncRead + AsyncWrite;

    /// Wraps the specified I/O object in the SSL/TLS stream.
    fn wrap(&self, io: T) -> Self::Wrapped;
}

/// The default `Tls` that returns the input I/O directly.
#[derive(Debug, Default)]
pub struct NoTls(());

impl<T> Tls<T> for NoTls
where
    T: AsyncRead + AsyncWrite,
{
    type Wrapped = T;
    type Wrapper = Self;

    #[inline]
    fn wrapper(&self, _: TlsConfig) -> crate::Result<Self::Wrapper> {
        Ok(NoTls(()))
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

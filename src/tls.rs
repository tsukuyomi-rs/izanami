//! SSL/TLS support.

#[path = "tls/native_tls.rs"]
mod native_tls;
mod openssl;
mod rustls;

use {
    futures::Future,
    izanami_util::http::SniHostname,
    tokio::io::{AsyncRead, AsyncWrite},
};

/// Trait representing a SSL/TLS configuration.
pub trait TlsConfig<T> {
    type Wrapped: AsyncRead + AsyncWrite;
    type Wrapper: TlsWrapper<T, Wrapped = Self::Wrapped> + Clone;

    /// Creates an instance of `Acceptor` using the specified configuration.
    fn into_wrapper(self, alpn_protocols: Vec<String>) -> crate::Result<Self::Wrapper>;
}

/// Trait representing a converter for granting the SSL/TLS to asynchronous I/Os.
pub trait TlsWrapper<T> {
    /// The type of wrapped asynchronous I/O returned from `accept`.
    type Wrapped: AsyncRead + AsyncWrite;

    type Error: Into<crate::error::BoxedStdError>;

    type Future: Future<Item = (Self::Wrapped, SniHostname), Error = Self::Error>;

    /// Wraps the specified I/O object in the SSL/TLS stream.
    fn wrap(&self, io: T) -> Self::Future;
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
    type Error = std::io::Error;
    type Future = futures::future::FutureResult<(Self::Wrapped, SniHostname), Self::Error>;

    #[inline]
    fn wrap(&self, io: T) -> Self::Future {
        futures::future::ok((io, SniHostname::unknown()))
    }
}

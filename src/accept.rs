#[path = "accept/native_tls.rs"]
mod navite_tls;
mod openssl;
mod rustls;

use tokio::io::{AsyncRead, AsyncWrite};

/// A trait that represents the conversion of asynchronous I/Os.
///
/// Typically, the implementors of this trait establish a TLS session.
pub trait Acceptor<T> {
    type Accepted: AsyncRead + AsyncWrite;

    /// Converts the supplied I/O object into an `Accepted`.
    ///
    /// The returned I/O from this method includes the handshake process,
    /// and the process will be executed by reading/writing the I/O.
    fn accept(&self, io: T) -> Self::Accepted;
}

impl<F, T, U> Acceptor<T> for F
where
    F: Fn(T) -> U,
    U: AsyncRead + AsyncWrite,
{
    type Accepted = U;

    #[inline]
    fn accept(&self, io: T) -> Self::Accepted {
        (*self)(io)
    }
}

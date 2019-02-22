//! SSL/TLS support.

use {
    futures::{Async, Future, Poll},
    tokio::io::{AsyncRead, AsyncWrite},
};

/// Trait representing a converter for granting the SSL/TLS to asynchronous I/Os.
pub trait MakeTlsTransport<T> {
    type Transport: AsyncRead + AsyncWrite;
    type Error;
    type Future: Future<Item = Self::Transport, Error = Self::Error>;

    #[doc(hidden)]
    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn make_transport(&mut self, target: T) -> Self::Future;
}

/// The default `TlsConfig` that returns the input I/O directly.
#[derive(Debug, Default, Clone)]
pub struct NoTls(());

impl<T> MakeTlsTransport<T> for NoTls
where
    T: AsyncRead + AsyncWrite,
{
    type Transport = T;
    type Error = std::io::Error;
    type Future = futures::future::FutureResult<Self::Transport, Self::Error>;

    #[inline]
    fn make_transport(&mut self, target: T) -> Self::Future {
        futures::future::ok(target)
    }
}

#[cfg(feature = "native-tls")]
mod native_tls {
    use {
        super::*,
        native_tls_crate as native_tls,
        tokio_tls::{Accept, TlsAcceptor, TlsStream},
    };

    impl<T> MakeTlsTransport<T> for TlsAcceptor
    where
        T: AsyncRead + AsyncWrite,
    {
        type Transport = TlsStream<T>;
        type Error = native_tls::Error;
        type Future = Accept<T>;

        #[inline]
        fn make_transport(&mut self, target: T) -> Self::Future {
            self.accept(target)
        }
    }
}

#[cfg(feature = "openssl")]
mod openssl {
    use {
        super::*,
        futures::{Future, Poll},
        openssl_crate::ssl::SslAcceptor,
        std::io,
        tokio::io::{AsyncRead, AsyncWrite},
        tokio_openssl::{SslAcceptorExt, SslStream},
    };

    impl<T> MakeTlsTransport<T> for SslAcceptor
    where
        T: AsyncRead + AsyncWrite,
    {
        type Transport = SslStream<T>;
        type Error = io::Error;
        type Future = AcceptAsync<T>;

        #[inline]
        fn make_transport(&mut self, target: T) -> Self::Future {
            AcceptAsync {
                inner: self.accept_async(target),
            }
        }
    }

    #[doc(hidden)]
    #[allow(missing_debug_implementations)]
    pub struct AcceptAsync<T: AsyncRead + AsyncWrite> {
        inner: tokio_openssl::AcceptAsync<T>,
    }

    impl<T> Future for AcceptAsync<T>
    where
        T: AsyncRead + AsyncWrite,
    {
        type Item = SslStream<T>;
        type Error = io::Error;

        fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
            self.inner
                .poll()
                .map_err(|_e| io::Error::new(io::ErrorKind::Other, "OpenSSL handshake error"))
        }
    }
}

#[cfg(feature = "rustls")]
mod rustls {
    use {
        super::*,
        rustls_crate::ServerSession,
        std::io,
        tokio_rustls::{Accept, TlsAcceptor, TlsStream},
    };

    impl<T> MakeTlsTransport<T> for TlsAcceptor
    where
        T: AsyncRead + AsyncWrite,
    {
        type Transport = TlsStream<T, ServerSession>;
        type Error = io::Error;
        type Future = Accept<T>;

        #[inline]
        fn make_transport(&mut self, target: T) -> Self::Future {
            self.accept(target)
        }
    }
}

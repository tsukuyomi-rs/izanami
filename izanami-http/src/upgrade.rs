//! HTTP upgrade abstraction.

use {
    crate::{
        body::{Body, HttpBody},
        conn::Connection,
    }, //
    futures::{Async, Poll},
    tokio_buf::{BufStream, SizeHint},
};

/// A trait that represents HTTP protocol upgrade.
pub trait HttpUpgrade<I> {
    /// The type of asynchronous process that drives upgraded protocol.
    type Upgraded: Connection<Error = Self::Error>;

    /// The error type that will be returned from the upgraded stream.
    type Error;

    /// Upgrades the specified stream to another protocol.
    ///
    /// When the value cannot upgrade the stream, the implementation
    /// of this method should return a `Err(stream)` to properly shut
    /// down the stream.
    fn upgrade(self, stream: I) -> Result<Self::Upgraded, I>;
}

impl<Bd, I> HttpUpgrade<I> for Bd
where
    Bd: BufStream,
{
    type Upgraded = futures::future::Empty<(), std::io::Error>;
    type Error = std::io::Error;

    fn upgrade(self, stream: I) -> Result<Self::Upgraded, I> {
        Err(stream)
    }
}

/// Creates an instance of `HttpUpgrade` using the provided function.
pub fn upgrade_fn<I, R>(
    f: impl FnOnce(I) -> Result<R, I>,
) -> impl HttpUpgrade<I, Upgraded = R, Error = R::Error>
where
    R: Connection,
{
    #[allow(missing_debug_implementations)]
    struct UpgradeFn<F>(F);

    impl<F, I, R> HttpUpgrade<I> for UpgradeFn<F>
    where
        F: FnOnce(I) -> Result<R, I>,
        R: Connection,
    {
        type Upgraded = R;
        type Error = R::Error;

        #[inline]
        fn upgrade(self, stream: I) -> Result<Self::Upgraded, I> {
            (self.0)(stream)
        }
    }

    UpgradeFn(f)
}

/// A wrapper for `HttpBody` for adding implementation of `HttpUpgrade`.
#[derive(Debug)]
pub struct NoUpgrade<T: HttpBody>(pub T);

impl<T> HttpBody for NoUpgrade<T>
where
    T: HttpBody,
{
    type Data = T::Data;
    type Error = T::Error;

    fn poll_data(&mut self) -> Poll<Option<Self::Data>, Self::Error> {
        self.0.poll_data()
    }

    fn size_hint(&self) -> SizeHint {
        self.0.size_hint()
    }

    fn poll_trailers(&mut self) -> Poll<Option<http::HeaderMap>, Self::Error> {
        self.0.poll_trailers()
    }

    fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }

    fn content_length(&self) -> Option<u64> {
        self.0.content_length()
    }
}

impl<T, I> HttpUpgrade<I> for NoUpgrade<T>
where
    T: HttpBody,
{
    type Upgraded = futures::future::Empty<(), std::io::Error>;
    type Error = std::io::Error;

    fn upgrade(self, stream: I) -> Result<Self::Upgraded, I> {
        Err(stream)
    }
}

/// An `HttpUpgrade` that contains either of an `HttpBody` or `HttpUpgrade`.
#[derive(Debug)]
pub enum MaybeUpgrade<T: HttpBody, U> {
    /// The body will be sent to the client without upgrading.
    Data(T),

    /// The body will be upgraded.
    Upgrade(U),
}

impl<T, U> HttpBody for MaybeUpgrade<T, U>
where
    T: HttpBody,
{
    type Data = T::Data;
    type Error = T::Error;

    fn poll_data(&mut self) -> Poll<Option<<Self as HttpBody>::Data>, Self::Error> {
        match self {
            MaybeUpgrade::Data(ref mut data) => data.poll_data(),
            MaybeUpgrade::Upgrade(..) => Ok(Async::Ready(None)),
        }
    }

    fn size_hint(&self) -> SizeHint {
        match self {
            MaybeUpgrade::Data(ref data) => data.size_hint(),
            MaybeUpgrade::Upgrade(..) => SizeHint::new(),
        }
    }

    fn poll_trailers(&mut self) -> Poll<Option<http::HeaderMap>, Self::Error> {
        match self {
            MaybeUpgrade::Data(ref mut data) => data.poll_trailers(),
            MaybeUpgrade::Upgrade(..) => Ok(Async::Ready(None)),
        }
    }

    fn is_end_stream(&self) -> bool {
        match self {
            MaybeUpgrade::Data(ref data) => data.is_end_stream(),
            MaybeUpgrade::Upgrade(..) => true,
        }
    }

    fn content_length(&self) -> Option<u64> {
        match self {
            MaybeUpgrade::Data(ref data) => data.content_length(),
            MaybeUpgrade::Upgrade(..) => None,
        }
    }
}

impl<T, U, I> HttpUpgrade<I> for MaybeUpgrade<T, U>
where
    T: HttpBody,
    U: HttpUpgrade<I>,
{
    type Upgraded = U::Upgraded;
    type Error = U::Error;

    fn upgrade(self, stream: I) -> Result<Self::Upgraded, I> {
        match self {
            MaybeUpgrade::Upgrade(cx) => cx.upgrade(stream),
            MaybeUpgrade::Data(..) => Err(stream),
        }
    }
}

impl<I> HttpUpgrade<I> for Body {
    type Upgraded = futures::future::Empty<(), Self::Error>;
    type Error = std::io::Error;

    fn upgrade(self, stream: I) -> Result<Self::Upgraded, I> {
        Err(stream)
    }
}

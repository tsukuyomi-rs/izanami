//! HTTP/1 upgrade.

#![allow(missing_docs)]

use {
    crate::body::HttpBody, //
    futures::{Async, Future, Poll},
    std::fmt,
    tokio_buf::{BufStream, SizeHint},
};

pub trait HttpUpgrade<I> {
    /// The type of asynchronous process that drives upgraded protocol.
    type Upgraded: Upgraded<Error = Self::Error>;

    /// The error type that will be returned from the upgraded stream.
    type Error;

    /// Upgrades the specified stream to another protocol.
    ///
    /// The implementation of this method may return a `Err(stream)`
    /// when the value cannot upgrade the stream.
    fn upgrade(self, stream: I) -> Result<Self::Upgraded, I>;
}

impl<Bd, I> HttpUpgrade<I> for Bd
where
    Bd: BufStream,
{
    type Upgraded = Never;
    type Error = Never;

    fn upgrade(self, stream: I) -> Result<Self::Upgraded, I> {
        Err(stream)
    }
}

pub trait Upgraded {
    type Error;

    fn poll_done(&mut self) -> Poll<(), Self::Error>;

    fn graceful_shutdown(&mut self) {}
}

impl<F> Upgraded for F
where
    F: Future<Item = ()>,
{
    type Error = F::Error;

    fn poll_done(&mut self) -> Poll<(), Self::Error> {
        Future::poll(self)
    }
}

pub enum Never {}

impl fmt::Debug for Never {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

impl fmt::Display for Never {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

impl std::error::Error for Never {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {}
    }
}

impl Upgraded for Never {
    type Error = Never;

    fn poll_done(&mut self) -> Poll<(), Self::Error> {
        match *self {}
    }

    fn graceful_shutdown(&mut self) {
        match *self {}
    }
}

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
    type Upgraded = Never;
    type Error = Never;

    fn upgrade(self, stream: I) -> Result<Self::Upgraded, I> {
        Err(stream)
    }
}

#[derive(Debug)]
pub enum MaybeUpgrade<T: HttpBody, U> {
    Data(T),
    Upgrade(U),
}

impl<T, U> HttpBody for MaybeUpgrade<T, U>
where
    T: HttpBody,
{
    type Data = T::Data;
    type Error = T::Error;

    fn poll_data(&mut self) -> Poll<Option<Self::Data>, Self::Error> {
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

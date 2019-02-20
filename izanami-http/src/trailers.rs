//! trailer headers.

use {
    futures::{Async, Future, Poll},
    http::HeaderMap,
};

/// A trait representing that it is possible that the stream
/// will return a `HeaderMap` after completing the output of bytes.
pub trait BodyTrailers {
    /// The type of errors that will be returned from `poll_trailers`.
    type TrailersError;

    /// Polls if this stream is ready to return a `HeaderMap`.
    fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::TrailersError> {
        Ok(Async::Ready(None))
    }

    /// Consumes itself and create a `Future` that polls the trailing headers.
    fn trailers(self) -> Trailers<Self>
    where
        Self: Sized,
    {
        Trailers { body: self }
    }
}

macro_rules! impl_has_trailers {
    ($($t:ty,)*) => {$(
        impl BodyTrailers for $t {
            type TrailersError = std::io::Error; // FIXME: replace with `!`
        }
    )*};
}

impl_has_trailers! {
    String,
    &'static str,
    Vec<u8>,
    &'static [u8],
    std::borrow::Cow<'static, str>,
    std::borrow::Cow<'static, [u8]>,
    bytes::Bytes,
    bytes::BytesMut,
}

/// A `Future` that polls the trailing headers.
#[derive(Debug)]
pub struct Trailers<Bd> {
    body: Bd,
}

impl<Bd> Future for Trailers<Bd>
where
    Bd: BodyTrailers,
{
    type Item = Option<HeaderMap>;
    type Error = Bd::TrailersError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.body.poll_trailers()
    }
}

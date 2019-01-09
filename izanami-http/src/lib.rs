//! Basic abstractions around HTTP used within izanami.

#![doc(html_root_url = "https://docs.rs/izanami-http/0.1.0-preview.1")]
#![deny(
    missing_docs,
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]

mod util;

use {
    bytes::Buf,
    either::Either,
    futures::{Async, Future, Poll},
    http::Request,
    std::error::Error,
    tokio_io::{AsyncRead, AsyncWrite},
};

/// A trait which abstracts an asynchronous stream of bytes.
///
/// The purpose of this trait is to imitate the trait defined in
/// (unreleased) `tokio-buf`, and it will be replaced by it in the future.
#[allow(missing_docs)]
pub trait BufStream {
    type Item: Buf;
    type Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error>;

    fn is_end_stream(&self) -> bool;
}

mod buf_stream {
    use {
        super::*,
        std::{borrow::Cow, io},
    };

    impl BufStream for String {
        type Item = io::Cursor<Vec<u8>>;
        type Error = io::Error;

        fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            if self.is_end_stream() {
                return Ok(Async::Ready(None));
            }

            let bytes = std::mem::replace(self, String::new()).into_bytes();
            Ok(Async::Ready(Some(io::Cursor::new(bytes))))
        }

        fn is_end_stream(&self) -> bool {
            self.is_empty()
        }
    }

    impl BufStream for Vec<u8> {
        type Item = io::Cursor<Vec<u8>>;
        type Error = io::Error;

        fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            if self.is_end_stream() {
                return Ok(Async::Ready(None));
            }

            let bytes = std::mem::replace(self, Vec::new());
            Ok(Async::Ready(Some(io::Cursor::new(bytes))))
        }

        fn is_end_stream(&self) -> bool {
            self.is_empty()
        }
    }

    impl<'a> BufStream for &'a str {
        type Item = io::Cursor<&'a [u8]>;
        type Error = io::Error;

        fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            if self.is_end_stream() {
                return Ok(Async::Ready(None));
            }

            let bytes = std::mem::replace(self, "").as_bytes();
            Ok(Async::Ready(Some(io::Cursor::new(bytes))))
        }

        fn is_end_stream(&self) -> bool {
            self.is_empty()
        }
    }

    impl<'a> BufStream for &'a [u8] {
        type Item = io::Cursor<&'a [u8]>;
        type Error = io::Error;

        fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            if self.is_end_stream() {
                return Ok(Async::Ready(None));
            }

            let bytes = std::mem::replace(self, &[]);
            Ok(Async::Ready(Some(io::Cursor::new(bytes))))
        }

        fn is_end_stream(&self) -> bool {
            self.is_empty()
        }
    }

    impl<'a> BufStream for Cow<'a, str> {
        type Item = io::Cursor<Cow<'a, [u8]>>;
        type Error = io::Error;

        fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            if self.is_end_stream() {
                return Ok(Async::Ready(None));
            }

            let bytes = match std::mem::replace(self, Cow::Borrowed("")) {
                Cow::Borrowed(borrowed) => Cow::Borrowed(borrowed.as_bytes()),
                Cow::Owned(owned) => Cow::Owned(owned.into_bytes()),
            };
            Ok(Async::Ready(Some(io::Cursor::new(bytes))))
        }

        fn is_end_stream(&self) -> bool {
            self.is_empty()
        }
    }

    impl<'a> BufStream for Cow<'a, [u8]> {
        type Item = io::Cursor<Cow<'a, [u8]>>;
        type Error = io::Error;

        fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            if self.is_end_stream() {
                return Ok(Async::Ready(None));
            }

            let bytes = std::mem::replace(self, Cow::Borrowed(&[]));
            Ok(Async::Ready(Some(io::Cursor::new(bytes))))
        }

        fn is_end_stream(&self) -> bool {
            self.is_empty()
        }
    }

    impl BufStream for bytes::Bytes {
        type Item = io::Cursor<bytes::Bytes>;
        type Error = io::Error;

        fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            if self.is_end_stream() {
                return Ok(Async::Ready(None));
            }

            let bytes = std::mem::replace(self, Default::default());
            Ok(Async::Ready(Some(io::Cursor::new(bytes))))
        }

        fn is_end_stream(&self) -> bool {
            self.is_empty()
        }
    }

    impl BufStream for bytes::BytesMut {
        type Item = io::Cursor<bytes::BytesMut>;
        type Error = io::Error;

        fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            if self.is_end_stream() {
                return Ok(Async::Ready(None));
            }

            let bytes = std::mem::replace(self, Default::default());
            Ok(Async::Ready(Some(io::Cursor::new(bytes))))
        }

        fn is_end_stream(&self) -> bool {
            self.is_empty()
        }
    }
}

/// A trait representing the conversion into a `BufStream`.
#[allow(missing_docs)]
pub trait IntoBufStream {
    type Item: Buf;
    type Error;
    type Stream: BufStream<Item = Self::Item, Error = Self::Error>;

    fn into_buf_stream(self) -> Self::Stream;
}

impl<T> IntoBufStream for T
where
    T: BufStream,
{
    type Item = T::Item;
    type Error = T::Error;
    type Stream = T;

    fn into_buf_stream(self) -> Self::Stream {
        self
    }
}

mod oneshot {
    use super::*;
    use std::io;

    impl IntoBufStream for () {
        type Item = io::Cursor<[u8; 0]>;
        type Error = io::Error;
        type Stream = Unit;

        fn into_buf_stream(self) -> Self::Stream {
            Unit {
                is_end_stream: false,
            }
        }
    }

    #[allow(missing_debug_implementations)]
    pub struct Unit {
        is_end_stream: bool,
    }

    impl BufStream for Unit {
        type Item = io::Cursor<[u8; 0]>;
        type Error = io::Error;

        fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            if !self.is_end_stream {
                self.is_end_stream = true;
                return Ok(Async::Ready(Some(io::Cursor::new([]))));
            }
            Ok(Async::Ready(None))
        }

        fn is_end_stream(&self) -> bool {
            self.is_end_stream
        }
    }
}

mod impl_either {
    use super::*;
    use crate::util::*;

    impl<L, R> IntoBufStream for Either<L, R>
    where
        L: IntoBufStream,
        R: IntoBufStream,
        L::Error: Into<Box<dyn Error + Send + Sync + 'static>>,
        R::Error: Into<Box<dyn Error + Send + Sync + 'static>>,
    {
        type Item = EitherBuf<L::Item, R::Item>;
        type Error = Box<dyn Error + Send + Sync + 'static>;
        type Stream = EitherStream<L::Stream, R::Stream>;

        fn into_buf_stream(self) -> Self::Stream {
            match self {
                Either::Left(l) => EitherStream::Left(l.into_buf_stream()),
                Either::Right(r) => EitherStream::Right(r.into_buf_stream()),
            }
        }
    }

    #[allow(missing_debug_implementations)]
    pub enum EitherStream<L, R> {
        Left(L),
        Right(R),
    }

    impl<L, R> BufStream for EitherStream<L, R>
    where
        L: BufStream,
        R: BufStream,
        L::Error: Into<Box<dyn Error + Send + Sync + 'static>>,
        R::Error: Into<Box<dyn Error + Send + Sync + 'static>>,
    {
        type Item = EitherBuf<L::Item, R::Item>;
        type Error = Box<dyn Error + Send + Sync + 'static>;

        fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            match self {
                EitherStream::Left(l) => l
                    .poll_buf()
                    .map_async_opt(EitherBuf::Left)
                    .map_err(Into::into),
                EitherStream::Right(r) => r
                    .poll_buf()
                    .map_async_opt(EitherBuf::Right)
                    .map_err(Into::into),
            }
        }

        fn is_end_stream(&self) -> bool {
            match self {
                EitherStream::Left(l) => l.is_end_stream(),
                EitherStream::Right(r) => r.is_end_stream(),
            }
        }
    }

    #[allow(missing_debug_implementations)]
    pub enum EitherBuf<L, R> {
        Left(L),
        Right(R),
    }

    impl<L, R> Buf for EitherBuf<L, R>
    where
        L: Buf,
        R: Buf,
    {
        fn remaining(&self) -> usize {
            match self {
                EitherBuf::Left(l) => l.remaining(),
                EitherBuf::Right(r) => r.remaining(),
            }
        }

        fn bytes(&self) -> &[u8] {
            match self {
                EitherBuf::Left(l) => l.bytes(),
                EitherBuf::Right(r) => r.bytes(),
            }
        }

        fn advance(&mut self, cnt: usize) {
            match self {
                EitherBuf::Left(l) => l.advance(cnt),
                EitherBuf::Right(r) => r.advance(cnt),
            }
        }
    }
}

#[allow(missing_docs)]
pub trait Upgradable {
    type Upgraded: AsyncRead + AsyncWrite;
    type Error;
    type OnUpgrade: Future<Item = Self::Upgraded, Error = Self::Error>;

    fn on_upgrade(self) -> Self::OnUpgrade;
}

impl<T> Upgradable for Request<T>
where
    T: Upgradable,
{
    type Upgraded = T::Upgraded;
    type Error = T::Error;
    type OnUpgrade = T::OnUpgrade;

    fn on_upgrade(self) -> Self::OnUpgrade {
        self.into_body().on_upgrade()
    }
}

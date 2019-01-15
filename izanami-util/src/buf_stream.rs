//! Components that abstracts asynchronous stream of bytes.
//!
//! This module is a simplified version of [`tower_web::util::buf_stream`]
//! and it will be *completely* replaced with `tokio-buf` as soon as
//! releasing this crate to crates.io.
//!
//! [`tower_web::util::buf_stream`]: https://docs.rs/tower-web/0.3/tower_web/util/buf_stream/index.html

#![allow(missing_docs)]

use {
    bytes::Buf,
    futures::{Async, Poll},
    std::error::Error,
};

/// A trait which abstracts an asynchronous stream of bytes.
pub trait BufStream {
    type Item: Buf;
    type Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error>;

    fn size_hint(&self) -> SizeHint {
        SizeHint::default()
    }

    #[allow(clippy::drop_copy)]
    fn consume_hint(&mut self, amount: usize) {
        drop(amount);
    }
}

#[derive(Debug, Default)]
pub struct SizeHint {
    lower: u64,
    upper: Option<u64>,
}

impl SizeHint {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn lower(&self) -> u64 {
        self.lower
    }

    pub fn upper(&self) -> Option<u64> {
        self.upper
    }

    pub fn set_lower(&mut self, value: u64) {
        assert!(value <= self.upper.unwrap_or(std::u64::MAX));
        self.lower = value;
    }

    pub fn set_upper(&mut self, value: u64) {
        assert!(value >= self.lower);
        self.upper = Some(value);
    }
}

mod impl_buf_stream {
    use {
        super::*,
        std::{borrow::Cow, io},
    };

    impl BufStream for String {
        type Item = io::Cursor<Vec<u8>>;
        type Error = io::Error;

        fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            if self.is_empty() {
                return Ok(Async::Ready(None));
            }

            let bytes = std::mem::replace(self, String::new()).into_bytes();
            Ok(Async::Ready(Some(io::Cursor::new(bytes))))
        }
    }

    impl BufStream for Vec<u8> {
        type Item = io::Cursor<Vec<u8>>;
        type Error = io::Error;

        fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            if self.is_empty() {
                return Ok(Async::Ready(None));
            }

            let bytes = std::mem::replace(self, Vec::new());
            Ok(Async::Ready(Some(io::Cursor::new(bytes))))
        }
    }

    impl<'a> BufStream for &'a str {
        type Item = io::Cursor<&'a [u8]>;
        type Error = io::Error;

        fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            if self.is_empty() {
                return Ok(Async::Ready(None));
            }

            let bytes = std::mem::replace(self, "").as_bytes();
            Ok(Async::Ready(Some(io::Cursor::new(bytes))))
        }
    }

    impl<'a> BufStream for &'a [u8] {
        type Item = io::Cursor<&'a [u8]>;
        type Error = io::Error;

        fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            if self.is_empty() {
                return Ok(Async::Ready(None));
            }

            let bytes = std::mem::replace(self, &[]);
            Ok(Async::Ready(Some(io::Cursor::new(bytes))))
        }
    }

    impl<'a> BufStream for Cow<'a, str> {
        type Item = io::Cursor<Cow<'a, [u8]>>;
        type Error = io::Error;

        fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            if self.is_empty() {
                return Ok(Async::Ready(None));
            }

            let bytes = match std::mem::replace(self, Cow::Borrowed("")) {
                Cow::Borrowed(borrowed) => Cow::Borrowed(borrowed.as_bytes()),
                Cow::Owned(owned) => Cow::Owned(owned.into_bytes()),
            };
            Ok(Async::Ready(Some(io::Cursor::new(bytes))))
        }
    }

    impl<'a> BufStream for Cow<'a, [u8]> {
        type Item = io::Cursor<Cow<'a, [u8]>>;
        type Error = io::Error;

        fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            if self.is_empty() {
                return Ok(Async::Ready(None));
            }

            let bytes = std::mem::replace(self, Cow::Borrowed(&[]));
            Ok(Async::Ready(Some(io::Cursor::new(bytes))))
        }
    }

    impl BufStream for bytes::Bytes {
        type Item = io::Cursor<bytes::Bytes>;
        type Error = io::Error;

        fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            if self.is_empty() {
                return Ok(Async::Ready(None));
            }

            let bytes = std::mem::replace(self, Default::default());
            Ok(Async::Ready(Some(io::Cursor::new(bytes))))
        }
    }

    impl BufStream for bytes::BytesMut {
        type Item = io::Cursor<bytes::BytesMut>;
        type Error = io::Error;

        fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            if self.is_empty() {
                return Ok(Async::Ready(None));
            }

            let bytes = std::mem::replace(self, Default::default());
            Ok(Async::Ready(Some(io::Cursor::new(bytes))))
        }
    }
}

#[derive(Debug)]
pub enum Either<L, R> {
    Left(L),
    Right(R),
}

mod impl_either {
    use super::*;
    use crate::util::*;

    impl<L, R> BufStream for Either<L, R>
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
                Either::Left(l) => l
                    .poll_buf()
                    .map_async_opt(EitherBuf::Left)
                    .map_err(Into::into),
                Either::Right(r) => r
                    .poll_buf()
                    .map_async_opt(EitherBuf::Right)
                    .map_err(Into::into),
            }
        }

        fn size_hint(&self) -> SizeHint {
            match self {
                Either::Left(l) => l.size_hint(),
                Either::Right(r) => r.size_hint(),
            }
        }

        fn consume_hint(&mut self, amount: usize) {
            match self {
                Either::Left(l) => l.consume_hint(amount),
                Either::Right(r) => r.consume_hint(amount),
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

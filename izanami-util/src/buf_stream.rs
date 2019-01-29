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
    futures::{Async, Future, Poll},
    std::{error::Error, fmt},
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

    fn collect<T>(self) -> Collect<Self, T>
    where
        Self: Sized,
        T: FromBufStream<Self::Item>,
    {
        let builder = T::builder(&self.size_hint());
        Collect {
            stream: self,
            builder: Some(builder),
        }
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

#[derive(Debug)]
pub struct Collect<S, T>
where
    S: BufStream,
    T: FromBufStream<S::Item>,
{
    stream: S,
    builder: Option<T::Builder>,
}

impl<S, T> Future for Collect<S, T>
where
    S: BufStream,
    T: FromBufStream<S::Item>,
{
    type Item = T;
    type Error = CollectError<S::Error, T::Error>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        while let Some(mut buf) = futures::try_ready! {
            self.stream.poll_buf()//
                .map_err(|e| CollectError {
                    kind: CollectErrorKind::Stream(e),
                })
        } {
            let builder = self.builder.as_mut().expect("cannot poll after done");
            T::extend(builder, &mut buf, &self.stream.size_hint()) //
                .map_err(|e| CollectError {
                    kind: CollectErrorKind::Collect(e),
                })?;
        }

        let builder = self.builder.take().expect("cannot poll after done");
        let value = T::build(builder) //
            .map_err(|e| CollectError {
                kind: CollectErrorKind::Collect(e),
            })?;

        Ok(Async::Ready(value))
    }
}

#[derive(Debug)]
pub struct CollectError<S, T> {
    kind: CollectErrorKind<S, T>,
}

#[derive(Debug)]
enum CollectErrorKind<S, T> {
    Stream(S),
    Collect(T),
}

impl<S, T> fmt::Display for CollectError<S, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("collect error")
    }
}

impl<S, T> Error for CollectError<S, T>
where
    S: Error + 'static,
    T: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            CollectErrorKind::Stream(e) => Some(e),
            CollectErrorKind::Collect(e) => Some(e),
        }
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

/// Trait representing the conversion from a `BufStream`.
pub trait FromBufStream<T: Buf>: Sized {
    type Builder;
    type Error;

    fn builder(hint: &SizeHint) -> Self::Builder;

    fn extend(builder: &mut Self::Builder, buf: &mut T, hint: &SizeHint)
        -> Result<(), Self::Error>;

    fn build(build: Self::Builder) -> Result<Self, Self::Error>;
}

mod from_buf_stream {
    use {
        super::*,
        bytes::{Bytes, BytesMut},
    };

    #[derive(Debug)]
    pub struct CollectVec {
        buf: Vec<u8>,
    }

    impl CollectVec {
        fn new(_: &SizeHint) -> Self {
            Self { buf: Vec::new() }
        }

        fn extend<T: Buf>(&mut self, buf: &mut T, _: &SizeHint) -> Result<(), CollectVecError> {
            self.buf.extend_from_slice(buf.bytes());
            Ok(())
        }

        fn build(self) -> Result<Vec<u8>, CollectVecError> {
            Ok(self.buf)
        }
    }

    #[derive(Debug)]
    pub struct CollectVecError(());

    impl fmt::Display for CollectVecError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("collect vec error")
        }
    }

    impl Error for CollectVecError {}

    impl<T> FromBufStream<T> for Vec<u8>
    where
        T: Buf,
    {
        type Builder = CollectVec;
        type Error = CollectVecError;

        #[inline]
        fn builder(hint: &SizeHint) -> Self::Builder {
            CollectVec::new(hint)
        }

        #[inline]
        fn extend(
            builder: &mut Self::Builder,
            buf: &mut T,
            hint: &SizeHint,
        ) -> Result<(), Self::Error> {
            builder.extend(buf, hint)
        }

        #[inline]
        fn build(builder: Self::Builder) -> Result<Self, Self::Error> {
            builder.build()
        }
    }

    #[derive(Debug)]
    pub struct CollectBytes {
        buf: BytesMut,
    }

    impl CollectBytes {
        fn new(_hint: &SizeHint) -> Self {
            Self {
                buf: BytesMut::new(),
            }
        }

        fn extend<T: Buf>(&mut self, buf: &mut T, _: &SizeHint) -> Result<(), CollectBytesError> {
            self.buf.extend_from_slice(buf.bytes());
            Ok(())
        }

        fn build(self) -> Result<Bytes, CollectBytesError> {
            Ok(self.buf.freeze())
        }
    }

    #[derive(Debug)]
    pub struct CollectBytesError(());

    impl fmt::Display for CollectBytesError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("collect bytes error")
        }
    }

    impl Error for CollectBytesError {}

    impl<T> FromBufStream<T> for Bytes
    where
        T: Buf,
    {
        type Builder = CollectBytes;
        type Error = CollectBytesError;

        #[inline]
        fn builder(hint: &SizeHint) -> Self::Builder {
            CollectBytes::new(hint)
        }

        #[inline]
        fn extend(
            builder: &mut Self::Builder,
            buf: &mut T,
            hint: &SizeHint,
        ) -> Result<(), Self::Error> {
            builder.extend(buf, hint)
        }

        #[inline]
        fn build(builder: Self::Builder) -> Result<Self, Self::Error> {
            builder.build()
        }
    }
}

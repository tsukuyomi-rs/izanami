//!

mod util;

use {
    bytes::{Buf, Bytes},
    either::Either,
    futures::{Async, Poll},
    std::error::Error,
};

/// A trait which abstracts an asynchronous stream of bytes.
///
/// The purpose of this trait is to imitate the trait defined in
/// (unreleased) `tokio-buf`, and it will be replaced by it in the future.
pub trait BufStream {
    type Item: Buf;
    type Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error>;

    fn is_end_stream(&self) -> bool;
}

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
        type Stream = OneshotStream<Self::Item>;

        fn into_buf_stream(self) -> Self::Stream {
            OneshotStream {
                inner: Some(io::Cursor::new([])),
            }
        }
    }

    impl<T> IntoBufStream for io::Cursor<T>
    where
        T: AsRef<[u8]>,
    {
        type Item = Self;
        type Error = io::Error;
        type Stream = OneshotStream<Self::Item>;

        fn into_buf_stream(self) -> Self::Stream {
            OneshotStream { inner: Some(self) }
        }
    }

    macro_rules! impl_into_buf_stream {
        ($($t:ty,)*) => {$(
            impl IntoBufStream for $t {
                type Item = io::Cursor<Self>;
                type Error = io::Error;
                type Stream = OneshotStream<Self::Item>;

                fn into_buf_stream(self) -> Self::Stream {
                    OneshotStream {
                        inner: Some(io::Cursor::new(self)),
                    }
                }
            }
        )*};
    }

    impl_into_buf_stream! {
        String,
        &'static str,

        Vec<u8>,
        &'static [u8],
        std::borrow::Cow<'static, [u8]>,

        Bytes,
    }

    #[allow(missing_debug_implementations)]
    pub struct OneshotStream<T> {
        inner: Option<T>,
    }

    impl<T> BufStream for OneshotStream<T>
    where
        T: Buf,
    {
        type Item = T;
        type Error = io::Error;

        fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            Ok(Async::Ready(self.inner.take()))
        }

        fn is_end_stream(&self) -> bool {
            self.inner.is_none()
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

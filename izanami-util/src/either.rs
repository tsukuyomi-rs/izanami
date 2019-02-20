#![allow(missing_docs)]

#[derive(Debug)]
pub enum Either<L, R> {
    Left(L),
    Right(R),
}

mod impl_buf_stream {
    use super::*;
    use crate::util::*;
    use {
        bytes::Buf,
        futures::Poll,
        izanami_buf::{BufStream, SizeHint},
        std::error::Error,
    };

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

mod impl_has_trailers {
    use super::*;
    use {
        crate::http::HasTrailers, //
        futures::Poll,
        http::header::HeaderMap,
        std::error::Error,
    };

    impl<L, R> HasTrailers for Either<L, R>
    where
        L: HasTrailers,
        R: HasTrailers,
        L::TrailersError: Into<Box<dyn Error + Send + Sync + 'static>>,
        R::TrailersError: Into<Box<dyn Error + Send + Sync + 'static>>,
    {
        type TrailersError = Box<dyn Error + Send + Sync + 'static>;

        #[inline]
        fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::TrailersError> {
            match self {
                Either::Left(l) => l.poll_trailers().map_err(Into::into),
                Either::Right(r) => r.poll_trailers().map_err(Into::into),
            }
        }
    }
}

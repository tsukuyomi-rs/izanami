// FIXME: replace with mock_io::Mock

use {
    crate::error::BoxedStdError,
    bytes::{Buf, Bytes},
    futures::{Async, Future, Poll},
    http::{header::HeaderMap, Request, Response},
    izanami_service::{MakeService, Service},
    izanami_util::{
        buf_stream::{BufStream, SizeHint},
        http::{HasTrailers, Upgrade},
    },
    std::{cell::UnsafeCell, io, marker::PhantomData},
    tokio::io::{AsyncRead, AsyncWrite},
};

/// A type that emulates an asynchronous I/O upgraded from HTTP.
///
/// Currently, this type is equivalent to a pair of `io::Empty` and `io::Sink`.
#[derive(Debug)]
pub struct MockUpgraded {
    reader: io::Empty,
    writer: io::Sink,
    _anchor: PhantomData<UnsafeCell<()>>,
}

impl io::Read for MockUpgraded {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reader.read(buf)
    }
}

impl io::Write for MockUpgraded {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.write(buf)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

impl AsyncRead for MockUpgraded {}

impl AsyncWrite for MockUpgraded {
    #[inline]
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.writer.shutdown()
    }
}

/// A struct that represents the stream of chunks from client.
#[derive(Debug)]
pub struct MockRequestBody {
    inner: Inner,
    _anchor: PhantomData<UnsafeCell<()>>,
}

#[derive(Debug)]
enum Inner {
    Sized(Option<Bytes>),
    OnUpgrade { upgraded: bool },
}

impl MockRequestBody {
    pub fn sized(data: impl Into<Bytes>) -> Self {
        Self {
            inner: Inner::Sized(Some(data.into())),
            _anchor: PhantomData,
        }
    }
}

impl From<()> for MockRequestBody {
    fn from(_: ()) -> Self {
        Self::sized(Bytes::new())
    }
}

macro_rules! impl_from_for_sized_data {
    ($($t:ty,)*) => {$(
        impl From<$t> for MockRequestBody {
            fn from(data: $t) -> Self {
                Self::sized(data)
            }
        }
    )*};
}

impl_from_for_sized_data! {
    &'static [u8],
    &'static str,
    String,
    Vec<u8>,
    Bytes,
}

impl BufStream for MockRequestBody {
    type Item = io::Cursor<Bytes>;
    type Error = io::Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match &mut self.inner {
            Inner::Sized(chunk) => Ok(Async::Ready(chunk.take().map(io::Cursor::new))),
            Inner::OnUpgrade { .. } => panic!("the request body has already been upgraded"),
        }
    }

    fn size_hint(&self) -> SizeHint {
        match &self.inner {
            Inner::Sized(chunk) => {
                let mut hint = SizeHint::new();
                if let Some(chunk) = chunk {
                    let len = chunk.len() as u64;
                    hint.set_upper(len);
                    hint.set_lower(len);
                }
                hint
            }
            Inner::OnUpgrade { .. } => panic!("the request body has already been upgraded"),
        }
    }
}

impl HasTrailers for MockRequestBody {
    type TrailersError = io::Error;

    fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::TrailersError> {
        match &mut self.inner {
            Inner::Sized(chunk) => {
                if chunk.is_some() {
                    panic!("The content of request body has yet polled yet.");
                }
                Ok(Async::Ready(None))
            }
            Inner::OnUpgrade { .. } => panic!("the request body has already been upgraded"),
        }
    }
}

impl Upgrade for MockRequestBody {
    type Upgraded = MockUpgraded;
    type Error = io::Error;

    fn poll_upgrade(&mut self) -> Poll<Self::Upgraded, Self::Error> {
        loop {
            self.inner = match &mut self.inner {
                Inner::Sized(..) => Inner::OnUpgrade { upgraded: false },
                Inner::OnUpgrade { upgraded } => {
                    if *upgraded {
                        panic!("the body has already been upgraded");
                    }
                    *upgraded = true;
                    return Ok(Async::Ready(MockUpgraded {
                        reader: io::empty(),
                        writer: io::sink(),
                        _anchor: PhantomData,
                    }));
                }
            };
        }
    }
}

/// The type of context values passed by the test server, used within `MakeService`s.
#[derive(Debug)]
pub struct TestContext<'a> {
    _anchor: PhantomData<&'a std::rc::Rc<()>>,
}

impl<'a> TestContext<'a> {
    pub(crate) fn new() -> Self {
        TestContext {
            _anchor: PhantomData,
        }
    }
}

/// A trait that abstracts the service factory used by the test server.
pub trait MakeTestService: self::imp::MakeTestServiceImpl {}

pub(crate) mod imp {
    use super::*;

    pub trait MakeTestServiceImpl {
        type ResponseBody: ResponseBody;
        type Error: Into<BoxedStdError>;
        type Service: TestService<ResponseBody = Self::ResponseBody, Error = Self::Error>;
        type MakeError: Into<BoxedStdError>;
        type Future: Future<Item = Self::Service, Error = Self::MakeError>;

        fn make_service(&self, cx: TestContext<'_>) -> Self::Future;
    }

    impl<S, Bd, SvcErr, Svc, MkErr, Fut> MakeTestService for S
    where
        S: for<'a> MakeService<
            TestContext<'a>,
            Request<MockRequestBody>,
            Response = Response<Bd>,
            Error = SvcErr,
            Service = Svc,
            MakeError = MkErr,
            Future = Fut,
        >,
        Bd: ResponseBody,
        Svc: Service<Request<MockRequestBody>, Response = Response<Bd>, Error = SvcErr>,
        SvcErr: Into<BoxedStdError>,
        MkErr: Into<BoxedStdError>,
        Fut: Future<Item = Svc, Error = MkErr>,
    {
    }

    impl<S, Bd, SvcErr, Svc, MkErr, Fut> MakeTestServiceImpl for S
    where
        S: for<'a> MakeService<
            TestContext<'a>,
            Request<MockRequestBody>,
            Response = Response<Bd>,
            Error = SvcErr,
            Service = Svc,
            MakeError = MkErr,
            Future = Fut,
        >,
        Bd: ResponseBody,
        Svc: Service<Request<MockRequestBody>, Response = Response<Bd>, Error = SvcErr>,
        SvcErr: Into<BoxedStdError>,
        MkErr: Into<BoxedStdError>,
        Fut: Future<Item = Svc, Error = MkErr>,
    {
        type ResponseBody = Bd;
        type Error = SvcErr;
        type Service = Svc;
        type MakeError = MkErr;
        type Future = Fut;

        fn make_service(&self, cx: TestContext<'_>) -> Self::Future {
            MakeService::make_service(self, cx)
        }
    }

    #[doc(hidden)]
    pub trait TestService {
        type ResponseBody: ResponseBody;
        type Error: Into<BoxedStdError>;
        type Future: Future<Item = Response<Self::ResponseBody>, Error = Self::Error>;

        fn call(&mut self, request: Request<MockRequestBody>) -> Self::Future;
    }

    impl<S, Bd> TestService for S
    where
        S: Service<Request<MockRequestBody>, Response = Response<Bd>>,
        S::Error: Into<BoxedStdError>,
        Bd: ResponseBody,
    {
        type ResponseBody = Bd;
        type Error = S::Error;
        type Future = S::Future;

        fn call(&mut self, request: Request<MockRequestBody>) -> Self::Future {
            Service::call(self, request)
        }
    }

    pub trait ResponseBody {
        type Item: Buf;
        type Error: Into<BoxedStdError>;

        fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error>;

        fn size_hint(&self) -> SizeHint;
    }

    impl<T> ResponseBody for T
    where
        T: BufStream,
        T::Error: Into<BoxedStdError>,
    {
        type Item = T::Item;
        type Error = T::Error;

        fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            BufStream::poll_buf(self)
        }

        fn size_hint(&self) -> SizeHint {
            BufStream::size_hint(self)
        }
    }
}

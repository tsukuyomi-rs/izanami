use {
    bytes::Bytes,
    futures::{Async, Poll},
    http::{header::HeaderMap, Request},
    izanami_util::{
        buf_stream::{BufStream, SizeHint},
        http::{HasTrailers, Upgrade},
    },
    std::{cell::UnsafeCell, io, marker::PhantomData},
    tokio::io::{AsyncRead, AsyncWrite},
};

// FIXME: replace with mock_io::Mock

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
    fn new(data: impl Into<Bytes>) -> Self {
        Self {
            inner: Inner::Sized(Some(data.into())),
            _anchor: PhantomData,
        }
    }
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

/// A trait representing the input to the test server.
pub trait Input: imp::InputImpl {}

mod imp {
    use super::*;

    pub trait InputImpl {
        fn build_request(self) -> http::Result<Request<MockRequestBody>>;
    }

    impl<T, E> Input for Result<T, E>
    where
        T: Input,
        E: Into<http::Error>,
    {
    }

    impl<T, E> InputImpl for Result<T, E>
    where
        T: Input,
        E: Into<http::Error>,
    {
        fn build_request(self) -> http::Result<Request<MockRequestBody>> {
            self.map_err(Into::into)?.build_request()
        }
    }

    impl Input for http::request::Builder {}

    impl InputImpl for http::request::Builder {
        fn build_request(mut self) -> http::Result<Request<MockRequestBody>> {
            (&mut self).build_request()
        }
    }

    impl<'a> Input for &'a mut http::request::Builder {}

    impl<'a> InputImpl for &'a mut http::request::Builder {
        fn build_request(self) -> http::Result<Request<MockRequestBody>> {
            self.body(MockRequestBody::new(Bytes::new()))
        }
    }

    impl Input for Request<()> {}

    impl InputImpl for Request<()> {
        fn build_request(self) -> http::Result<Request<MockRequestBody>> {
            Ok(self.map(|_| MockRequestBody::new(Bytes::new())))
        }
    }

    impl<'a> Input for Request<&'a str> {}

    impl<'a> InputImpl for Request<&'a str> {
        fn build_request(self) -> http::Result<Request<MockRequestBody>> {
            Ok(self.map(MockRequestBody::new))
        }
    }

    impl Input for Request<String> {}

    impl InputImpl for Request<String> {
        fn build_request(self) -> http::Result<Request<MockRequestBody>> {
            Ok(self.map(MockRequestBody::new))
        }
    }

    impl<'a> Input for Request<&'a [u8]> {}

    impl<'a> InputImpl for Request<&'a [u8]> {
        fn build_request(self) -> http::Result<Request<MockRequestBody>> {
            Ok(self.map(MockRequestBody::new))
        }
    }

    impl Input for Request<Vec<u8>> {}

    impl InputImpl for Request<Vec<u8>> {
        fn build_request(self) -> http::Result<Request<MockRequestBody>> {
            Ok(self.map(MockRequestBody::new))
        }
    }

    impl Input for Request<Bytes> {}

    impl InputImpl for Request<Bytes> {
        fn build_request(self) -> http::Result<Request<MockRequestBody>> {
            Ok(self.map(MockRequestBody::new))
        }
    }

    impl<'a> Input for &'a str {}

    impl<'a> InputImpl for &'a str {
        fn build_request(self) -> http::Result<Request<MockRequestBody>> {
            Request::get(self) //
                .body(MockRequestBody::new(Bytes::new()))
        }
    }

    impl Input for String {}

    impl InputImpl for String {
        fn build_request(self) -> http::Result<Request<MockRequestBody>> {
            self.as_str().build_request()
        }
    }
}

#[cfg(test)]
mod test {
    use {
        super::{imp::InputImpl, *},
        http::Method,
    };

    #[test]
    fn input_string() -> http::Result<()> {
        let request = "/foo".build_request()?;
        assert_eq!(request.method(), Method::GET);
        assert_eq!(request.uri().path(), "/foo");
        assert!(request.headers().is_empty());
        Ok(())
    }

    #[test]
    fn input_request_bytes() -> http::Result<()> {
        let request = Request::get("/") //
            .body(Bytes::new())
            .build_request()?;
        assert_eq!(request.method(), Method::GET);
        assert_eq!(request.uri().path(), "/");
        assert!(!request.headers().contains_key("content-type"));
        Ok(())
    }

    #[test]
    fn input_request_str() -> http::Result<()> {
        let request = Request::get("/") //
            .body("hello, izanami")
            .build_request()?;
        assert_eq!(request.method(), Method::GET);
        assert_eq!(request.uri().path(), "/");
        assert!(!request.headers().contains_key("content-type"));
        Ok(())
    }

    #[test]
    fn input_request_slice() -> http::Result<()> {
        let request = Request::new(&b""[..]).build_request()?;
        assert_eq!(request.method(), Method::GET);
        assert_eq!(request.uri().path(), "/");
        assert!(!request.headers().contains_key("content-type"));
        Ok(())
    }
}

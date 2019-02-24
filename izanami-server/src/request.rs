//! HTTP request used in this crate.

use {
    crate::BoxedStdError,
    bytes::{Buf, BufMut, Bytes},
    futures::{Future, Poll},
    http::HeaderMap,
    hyper::body::Payload as _Payload,
    izanami_http::{body::ContentLength, BodyTrailers, HttpBody, Upgrade},
    izanami_util::*,
    std::io,
    tokio::io::{AsyncRead, AsyncWrite},
    tokio_buf::{BufStream, SizeHint},
};

/// The message body of an incoming HTTP request.
#[derive(Debug)]
pub struct RequestBody(hyper::Body);

impl RequestBody {
    pub(crate) fn from_hyp(body: hyper::Body) -> Self {
        RequestBody(body)
    }

    /// Returns whether the body is complete or not.
    pub fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }

    /// Returns a length of the total bytes, if possible.
    pub fn content_length(&self) -> Option<u64> {
        self.0.content_length()
    }
}

impl BufStream for RequestBody {
    type Item = io::Cursor<Bytes>;
    type Error = BoxedStdError;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.0
            .poll_data()
            .map(|x| x.map(|opt| opt.map(|chunk| io::Cursor::new(chunk.into_bytes()))))
            .map_err(Into::into)
    }

    fn size_hint(&self) -> SizeHint {
        let mut hint = SizeHint::new();
        if let Some(len) = self.0.content_length() {
            hint.set_upper(len);
            hint.set_lower(len);
        }
        hint
    }
}

impl BodyTrailers for RequestBody {
    type TrailersError = BoxedStdError;

    fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::TrailersError> {
        self.0.poll_trailers().map_err(Into::into)
    }
}

impl HttpBody for RequestBody {
    fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }

    fn content_length(&self) -> ContentLength {
        match self.0.content_length() {
            Some(len) => ContentLength::Sized(len),
            None => ContentLength::Chunked,
        }
    }
}

impl Upgrade for RequestBody {
    type Upgraded = Upgraded;
    type Error = BoxedStdError;
    type Future = OnUpgrade;

    fn on_upgrade(self) -> Self::Future {
        OnUpgrade(self.0.on_upgrade())
    }
}

#[derive(Debug)]
pub struct OnUpgrade(hyper::upgrade::OnUpgrade);

impl Future for OnUpgrade {
    type Item = Upgraded;
    type Error = BoxedStdError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.0.poll().map_async(Upgraded).map_err(Into::into)
    }
}

/// Asyncrhonous I/O object used after upgrading the protocol from HTTP.
#[derive(Debug)]
pub struct Upgraded(hyper::upgrade::Upgraded);

impl Upgraded {
    /// Acquire the instance of actual I/O object that has
    /// the specified concrete type, if available.
    #[inline]
    pub fn downcast<T>(self) -> Result<(T, Bytes), Self>
    where
        T: AsyncRead + AsyncWrite + 'static,
    {
        self.0
            .downcast::<T>()
            .map(|parts| (parts.io, parts.read_buf))
            .map_err(Upgraded)
    }
}

impl io::Read for Upgraded {
    #[inline]
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        self.0.read(dst)
    }
}

impl io::Write for Upgraded {
    #[inline]
    fn write(&mut self, src: &[u8]) -> io::Result<usize> {
        self.0.write(src)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl AsyncRead for Upgraded {
    #[inline]
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.0.prepare_uninitialized_buffer(buf)
    }

    #[inline]
    fn read_buf<B: BufMut>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        self.0.read_buf(buf)
    }
}

impl AsyncWrite for Upgraded {
    #[inline]
    fn write_buf<B: Buf>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        self.0.write_buf(buf)
    }

    #[inline]
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.0.shutdown()
    }
}

/// Type alias representing the HTTP request passed to the services.
pub type HttpRequest = http::Request<RequestBody>;

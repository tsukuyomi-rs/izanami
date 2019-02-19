//! Abstraction around HTTP services.

use {
    crate::error::BoxedStdError,
    bytes::{Buf, BufMut, Bytes},
    futures::{Async, Future, Poll},
    http::HeaderMap,
    hyper::body::Payload as _Payload,
    izanami_service::Service,
    izanami_util::{
        buf_stream::{BufStream, SizeHint},
        http::{HasTrailers, Upgrade},
    },
    std::io,
    tokio::io::{AsyncRead, AsyncWrite},
};

/// The message body of an incoming HTTP request.
#[derive(Debug)]
pub struct RequestBody(Inner);

#[derive(Debug)]
enum Inner {
    Stream(hyper::Body),
    OnUpgrade(hyper::upgrade::OnUpgrade),
}

impl RequestBody {
    pub(crate) fn from_hyp(body: hyper::Body) -> Self {
        RequestBody(Inner::Stream(body))
    }

    /// Returns whether the body is complete or not.
    pub fn is_end_stream(&self) -> bool {
        match &self.0 {
            Inner::Stream(body) => body.is_end_stream(),
            _ => true,
        }
    }

    /// Returns whether this stream has already been upgraded.
    ///
    /// If this method returns `true`, the result from `BufStream::poll_buf`
    /// or `HasTrailers::poll_trailers` becomes an error.
    pub fn is_upgraded(&self) -> bool {
        match self.0 {
            Inner::OnUpgrade(..) => true,
            _ => false,
        }
    }

    /// Returns a length of the total bytes, if possible.
    pub fn content_length(&self) -> Option<u64> {
        match &self.0 {
            Inner::Stream(body) => body.content_length(),
            _ => None,
        }
    }
}

impl BufStream for RequestBody {
    type Item = io::Cursor<Bytes>;
    type Error = BoxedStdError;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match &mut self.0 {
            Inner::Stream(body) => body
                .poll_data()
                .map(|x| x.map(|opt| opt.map(|chunk| io::Cursor::new(chunk.into_bytes()))))
                .map_err(Into::into),
            Inner::OnUpgrade(..) => Err(already_upgraded()),
        }
    }

    fn size_hint(&self) -> SizeHint {
        match &self.0 {
            Inner::Stream(body) => {
                let mut hint = SizeHint::new();
                if let Some(len) = body.content_length() {
                    hint.set_upper(len);
                    hint.set_lower(len);
                }
                hint
            }
            Inner::OnUpgrade(..) => SizeHint::new(),
        }
    }
}

impl HasTrailers for RequestBody {
    type TrailersError = BoxedStdError;

    fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::TrailersError> {
        match &mut self.0 {
            Inner::Stream(body) => body.poll_trailers().map_err(Into::into),
            Inner::OnUpgrade(..) => Err(already_upgraded()),
        }
    }
}

impl Upgrade for RequestBody {
    type Upgraded = Upgraded;
    type Error = BoxedStdError;

    fn poll_upgrade(&mut self) -> Poll<Self::Upgraded, Self::Error> {
        loop {
            self.0 = match &mut self.0 {
                Inner::Stream(body) => {
                    let body = std::mem::replace(body, hyper::Body::empty());
                    Inner::OnUpgrade(body.on_upgrade())
                }
                Inner::OnUpgrade(on_upgrade) => {
                    return on_upgrade
                        .poll()
                        .map(|x| x.map(Upgraded))
                        .map_err(Into::into);
                }
            };
        }
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

pub trait NewHttpService<T> {
    type Response: HttpResponse;
    type Error: Into<BoxedStdError>;
    type Service: HttpService<Response = Self::Response, Error = Self::Error>;
    type IntoService: IntoHttpService<
        T,
        Response = Self::Response,
        Error = Self::Error,
        Service = Self::Service,
    >;
    type MakeError: Into<BoxedStdError>;
    type Future: Future<Item = Self::IntoService, Error = Self::MakeError>;

    #[doc(hidden)]
    fn poll_ready(&mut self) -> Poll<(), Self::MakeError> {
        Ok(Async::Ready(()))
    }

    fn new_service(&mut self) -> Self::Future;
}

impl<S, T> NewHttpService<T> for S
where
    S: Service<()>,
    S::Response: IntoHttpService<T>,
    S::Error: Into<BoxedStdError>,
{
    type Response = <S::Response as IntoHttpService<T>>::Response;
    type Error = <S::Response as IntoHttpService<T>>::Error;
    type Service = <S::Response as IntoHttpService<T>>::Service;
    type IntoService = S::Response;
    type MakeError = S::Error;
    type Future = S::Future;

    #[inline]
    fn poll_ready(&mut self) -> Poll<(), Self::MakeError> {
        Service::poll_ready(self)
    }

    #[inline]
    fn new_service(&mut self) -> Self::Future {
        Service::call(self, ())
    }
}

pub trait IntoHttpService<T> {
    type Response: HttpResponse;
    type Error: Into<BoxedStdError>;
    type Service: HttpService<Response = Self::Response, Error = Self::Error>;

    fn into_service(self, target: &T) -> Self::Service;
}

impl<S, T> IntoHttpService<T> for S
where
    S: HttpService,
{
    type Response = S::Response;
    type Error = S::Error;
    type Service = S;

    #[inline]
    fn into_service(self, _: &T) -> Self::Service {
        self
    }
}

pub trait HttpService {
    type Response: HttpResponse;
    type Error: Into<BoxedStdError>;
    type Future: Future<Item = Self::Response, Error = Self::Error>;

    #[doc(hidden)]
    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn respond(&mut self, request: HttpRequest) -> Self::Future;
}

impl<S> HttpService for S
where
    S: Service<HttpRequest>,
    S::Response: HttpResponse,
    S::Error: Into<BoxedStdError>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    #[inline]
    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Service::poll_ready(self)
    }

    #[inline]
    fn respond(&mut self, request: HttpRequest) -> Self::Future {
        Service::call(self, request)
    }
}

pub trait HttpResponse: imp::HttpResponseImpl {}

impl<Bd> HttpResponse for http::Response<Bd> where Bd: ResponseBody {}

pub trait ResponseBody: imp::ResponseBodyImpl {}

impl<Bd> ResponseBody for Bd
where
    Bd: BufStream + HasTrailers + Send + 'static,
    Bd::Item: Send,
    Bd::Error: Into<BoxedStdError>,
    Bd::TrailersError: Into<BoxedStdError>,
{
}

fn already_upgraded() -> BoxedStdError {
    failure::format_err!("the request body has already been upgraded")
        .compat()
        .into()
}

pub(crate) mod imp {
    use super::*;

    pub trait HttpResponseImpl {
        type Data: Buf + Send;
        type Body: ResponseBody<Data = Self::Data>;
        fn into_response(self) -> http::Response<Self::Body>;
    }

    impl<Bd> HttpResponseImpl for http::Response<Bd>
    where
        Bd: ResponseBody,
    {
        type Data = Bd::Data;
        type Body = Bd;

        #[inline]
        fn into_response(self) -> http::Response<Self::Body> {
            self
        }
    }

    pub trait ResponseBodyImpl: Send + 'static {
        type Data: Buf + Send;
        fn poll_data(&mut self) -> Poll<Option<Self::Data>, BoxedStdError>;
        fn poll_trailers(&mut self) -> Poll<Option<http::HeaderMap>, BoxedStdError>;
        fn size_hint(&self) -> SizeHint;
    }

    impl<Bd> ResponseBodyImpl for Bd
    where
        Bd: BufStream + HasTrailers + Send + 'static,
        Bd::Item: Send,
        Bd::Error: Into<BoxedStdError>,
        Bd::TrailersError: Into<BoxedStdError>,
    {
        type Data = Bd::Item;

        #[inline]
        fn poll_data(&mut self) -> Poll<Option<Self::Data>, BoxedStdError> {
            BufStream::poll_buf(self).map_err(Into::into)
        }

        #[inline]
        fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, BoxedStdError> {
            HasTrailers::poll_trailers(self).map_err(Into::into)
        }

        #[inline]
        fn size_hint(&self) -> SizeHint {
            BufStream::size_hint(self)
        }
    }
}

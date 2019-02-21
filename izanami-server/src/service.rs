//! Abstraction around HTTP services.

use {
    crate::{util::*, BoxedStdError},
    bytes::{Buf, BufMut, Bytes},
    futures::{Async, Future, Poll},
    http::HeaderMap,
    hyper::body::Payload as _Payload,
    izanami_buf::{BufStream, SizeHint},
    izanami_http::{BodyTrailers, Upgrade},
    izanami_service::{IntoService, Service, ServiceRef},
    std::io,
    tokio::io::{AsyncRead, AsyncWrite},
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

/// An asynchronous factory of `HttpService`s.
pub trait MakeHttpService<T1, T2>: self::imp::MakeHttpServiceSealed<T1, T2> {
    type Response: HttpResponse;
    type Error: Into<BoxedStdError>;
    type Service: HttpService<Response = Self::Response, Error = Self::Error>;
    type IntoService: IntoHttpService<
        T2,
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

    fn make_service(&mut self, target: &T1) -> Self::Future;
}

impl<S, T1, T2> MakeHttpService<T1, T2> for S
where
    S: ServiceRef<T1>,
    S::Response: IntoHttpService<T2>,
    S::Error: Into<BoxedStdError>,
{
    type Response = <S::Response as IntoHttpService<T2>>::Response;
    type Error = <S::Response as IntoHttpService<T2>>::Error;
    type Service = <S::Response as IntoHttpService<T2>>::Service;
    type IntoService = S::Response;
    type MakeError = S::Error;
    type Future = S::Future;

    #[inline]
    fn poll_ready(&mut self) -> Poll<(), Self::MakeError> {
        ServiceRef::poll_ready(self)
    }

    #[inline]
    fn make_service(&mut self, target: &T1) -> Self::Future {
        ServiceRef::call(self, target)
    }
}

pub trait IntoHttpService<T>: self::imp::IntoHttpServiceSealed<T> {
    type Response: HttpResponse;
    type Error: Into<BoxedStdError>;
    type Service: HttpService<Response = Self::Response, Error = Self::Error>;

    fn into_service(self, target: &T) -> Self::Service;
}

impl<S, T, Res, Err, Svc> IntoHttpService<T> for S
where
    S: for<'a> IntoService<&'a T, HttpRequest, Response = Res, Error = Err, Service = Svc>,
    Res: HttpResponse,
    Err: Into<BoxedStdError>,
    Svc: Service<HttpRequest, Response = Res, Error = Err>,
{
    type Response = Res;
    type Error = Err;
    type Service = Svc;

    #[inline]
    fn into_service(self, target: &T) -> Self::Service {
        IntoService::into_service(self, target)
    }
}

/// An asynchronous service that handles HTTP requests on a transport.
///
/// The implementation of this trait is automatically provided when the
/// type has an implementation of `Service`.
pub trait HttpService: self::imp::HttpServiceSealed {
    /// The type of HTTP response returned from `respond`.
    type Response: HttpResponse;

    /// The error type which will be returned from this service.
    ///
    /// Returning an error means that the server will abruptly abort
    /// the connection rather than sending 4xx or 5xx response.
    type Error: Into<BoxedStdError>;

    /// The future that handles an incoming HTTP request.
    type Future: Future<Item = Self::Response, Error = Self::Error>;

    #[doc(hidden)]
    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    /// Handles an incoming HTTP request and returns its response asynchronously.
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
    Bd: BufStream + BodyTrailers + Send + 'static,
    Bd::Item: Send,
    Bd::Error: Into<BoxedStdError>,
    Bd::TrailersError: Into<BoxedStdError>,
{
}

pub(crate) mod imp {
    use super::*;

    pub trait HttpServiceSealed {}

    impl<S> HttpServiceSealed for S
    where
        S: Service<HttpRequest>,
        S::Response: HttpResponse,
        S::Error: Into<BoxedStdError>,
    {
    }

    pub trait MakeHttpServiceSealed<T1, T2> {}

    impl<S, T1, T2> MakeHttpServiceSealed<T1, T2> for S
    where
        S: ServiceRef<T1>,
        S::Response: IntoHttpService<T2>,
        S::Error: Into<BoxedStdError>,
    {
    }

    pub trait IntoHttpServiceSealed<T> {}

    impl<S, T, Res, Err, Svc> IntoHttpServiceSealed<T> for S
    where
        S: for<'a> IntoService<&'a T, HttpRequest, Response = Res, Error = Err, Service = Svc>,
        Res: HttpResponse,
        Err: Into<BoxedStdError>,
        Svc: Service<HttpRequest, Response = Res, Error = Err>,
    {
    }

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
        Bd: BufStream + BodyTrailers + Send + 'static,
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
            BodyTrailers::poll_trailers(self).map_err(Into::into)
        }

        #[inline]
        fn size_hint(&self) -> SizeHint {
            BufStream::size_hint(self)
        }
    }
}

use {
    crate::body::HttpBody,
    futures::{Future, Poll},
    http::{Request, Response},
    izanami_service::Service,
};

/// An asynchronous service that handles HTTP requests on a transport.
pub trait HttpService<RequestBody>: Sealed<RequestBody>
where
    RequestBody: HttpBody,
{
    /// The type of HTTP response returned from `respond`.
    type ResponseBody: HttpBody;

    /// The error type which will be returned from this service.
    type Error;

    /// The future that handles an incoming HTTP request.
    type Future: Future<Item = Response<Self::ResponseBody>, Error = Self::Error>;

    #[doc(hidden)]
    fn poll_ready(&mut self) -> Poll<(), Self::Error>;

    /// Handles an incoming HTTP request and returns its response asynchronously.
    fn respond(&mut self, request: Request<RequestBody>) -> Self::Future;
}

impl<S, ReqBd, ResBd> HttpService<ReqBd> for S
where
    S: Service<Request<ReqBd>, Response = Response<ResBd>>,
    ReqBd: HttpBody,
    ResBd: HttpBody,
{
    type ResponseBody = ResBd;
    type Error = S::Error;
    type Future = S::Future;

    #[inline]
    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Service::poll_ready(self)
    }

    #[inline]
    fn respond(&mut self, request: Request<ReqBd>) -> Self::Future {
        Service::call(self, request)
    }
}

pub trait Sealed<RequestBody> {}

impl<S, ReqBd, ResBd> Sealed<ReqBd> for S where
    S: Service<
        Request<ReqBd>, //
        Response = Response<ResBd>,
    >
{
}

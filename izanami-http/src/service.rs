//! HTTP services.

use {
    crate::{body::HttpBody, ResponseBody},
    futures::{Future, Poll},
    http::{Request, Response},
    izanami_service::Service,
};

/// An asynchronous service that handles HTTP requests on a transport.
pub trait HttpService<RequestBody>: sealed::Sealed<RequestBody>
where
    RequestBody: HttpBody,
{
    /// The type of HTTP response returned from `respond`.
    type ResponseBody: ResponseBody;

    /// The error type which will be returned from this service.
    type Error;

    /// The future that handles an incoming HTTP request.
    type Respond: Future<Item = Response<Self::ResponseBody>, Error = Self::Error>;

    /// Returns `true` when the service is ready to call `respond`.
    fn poll_ready(&mut self) -> Poll<(), Self::Error>;

    /// Handles an incoming HTTP request and returns its response asynchronously.
    fn respond(&mut self, request: Request<RequestBody>) -> Self::Respond;
}

impl<S, ReqBd, ResBd> HttpService<ReqBd> for S
where
    S: Service<Request<ReqBd>, Response = Response<ResBd>>,
    ReqBd: HttpBody,
    ResBd: ResponseBody,
{
    type ResponseBody = ResBd;
    type Error = S::Error;
    type Respond = S::Future;

    #[inline]
    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Service::poll_ready(self)
    }

    #[inline]
    fn respond(&mut self, request: Request<ReqBd>) -> Self::Respond {
        Service::call(self, request)
    }
}

mod sealed {
    use super::*;

    pub trait Sealed<RequestBody> {}

    impl<S, ReqBd, ResBd> Sealed<ReqBd> for S where
        S: Service<
            Request<ReqBd>, //
            Response = Response<ResBd>,
        >
    {
    }
}

// ===== HttpServiceExt =====

/// An extension trait for providing some adaptors for `HttpService`s.
pub trait HttpServiceExt<RequestBody>: HttpService<RequestBody>
where
    RequestBody: HttpBody,
{
    /// Lift this value into a `Service`.
    fn into_service(self) -> IntoService<Self>
    where
        Self: Sized,
    {
        IntoService { inner: self }
    }
}

// ===== IntoService =====

/// A wrapper for `HttpService`s for lifting the instance into a `Service`.
#[derive(Debug)]
pub struct IntoService<S> {
    inner: S,
}

impl<S, RequestBody> Service<Request<RequestBody>> for IntoService<S>
where
    S: HttpService<RequestBody>,
    RequestBody: HttpBody,
{
    type Response = Response<S::ResponseBody>;
    type Error = S::Error;
    type Future = S::Respond;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.inner.poll_ready()
    }

    fn call(&mut self, request: Request<RequestBody>) -> Self::Future {
        self.inner.respond(request)
    }
}

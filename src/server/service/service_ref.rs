use {
    futures::{Future, Poll},
    tower_service::Service,
};

#[allow(missing_docs)]
pub trait ServiceRef<Request>: Sealed<Request> {
    type Response;
    type Error;
    type Future: Future<Item = Self::Response, Error = Self::Error>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error>;

    fn call(&mut self, request: &Request) -> Self::Future;
}

impl<S, Req, Res, Err, Fut> ServiceRef<Req> for S
where
    S: for<'a> Service<&'a Req, Response = Res, Error = Err, Future = Fut>,
    Fut: Future<Item = Res, Error = Err>,
{
    type Response = Res;
    type Error = Err;
    type Future = Fut;

    #[inline]
    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Service::poll_ready(self)
    }

    #[inline]
    fn call(&mut self, request: &Req) -> Self::Future {
        Service::call(self, request)
    }
}

pub trait Sealed<Request> {}

impl<S, Req> Sealed<Req> for S where S: for<'a> Service<&'a Req> {}

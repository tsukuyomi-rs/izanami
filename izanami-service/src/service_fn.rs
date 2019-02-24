use {
    crate::Service,
    futures::{Async, IntoFuture, Poll},
};

/// Creates a `Service` from a function that returns responses asynchronously.
pub fn service_fn<F, Req, R>(f: F) -> ServiceFn<F>
where
    F: FnMut(Req) -> R,
    R: IntoFuture,
{
    ServiceFn(f)
}

#[derive(Debug)]
pub struct ServiceFn<F>(F);

impl<F, Req, R> Service<Req> for ServiceFn<F>
where
    F: FnMut(Req) -> R,
    R: IntoFuture,
{
    type Response = R::Item;
    type Error = R::Error;
    type Future = R::Future;

    #[inline]
    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    #[inline]
    fn call(&mut self, request: Req) -> Self::Future {
        (self.0)(request).into_future()
    }
}

/// Creates a `Service` from a function that returns responses immediately.
// FIXME: use async fn
pub fn service_fn_ok<F, Req, Res>(f: F) -> ServiceFnOk<F>
where
    F: FnMut(Req) -> Res,
{
    ServiceFnOk(f)
}

#[derive(Debug)]
pub struct ServiceFnOk<F>(F);

impl<F, Req, Res> Service<Req> for ServiceFnOk<F>
where
    F: FnMut(Req) -> Res,
{
    type Response = Res;
    type Error = std::io::Error; // FIXME: replace with '!'
    type Future = futures::future::FutureResult<Self::Response, Self::Error>;

    #[inline]
    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    #[inline]
    fn call(&mut self, request: Req) -> Self::Future {
        futures::future::ok((self.0)(request))
    }
}

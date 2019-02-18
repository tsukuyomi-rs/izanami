use {
    crate::Service,
    futures::{Async, IntoFuture, Poll},
};

/// Creates a `Service` from the specified closure.
pub fn service_fn<F, Request, R>(f: F) -> ServiceFn<F>
where
    F: FnMut(Request) -> R,
    R: IntoFuture,
{
    ServiceFn(f)
}

#[derive(Debug)]
pub struct ServiceFn<F>(F);

impl<F, Request, R> Service<Request> for ServiceFn<F>
where
    F: FnMut(Request) -> R,
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
    fn call(&mut self, request: Request) -> Self::Future {
        (self.0)(request).into_future()
    }
}

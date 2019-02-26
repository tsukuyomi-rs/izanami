use {
    crate::Service,
    futures::{Async, IntoFuture, Poll},
    std::marker::PhantomData,
};

/// Creates a `Service` from a function that returns responses asynchronously.
pub fn service_fn<F, Req, R>(f: F) -> ServiceFn<F, Req, R>
where
    F: FnMut(Req) -> R,
    R: IntoFuture,
{
    ServiceFn {
        f,
        _marker: PhantomData,
    }
}

#[derive(Debug)]
pub struct ServiceFn<F, Req, R> {
    f: F,
    _marker: PhantomData<fn(Req) -> R>,
}

impl<F, Req, R> Service<Req> for ServiceFn<F, Req, R>
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
        (self.f)(request).into_future()
    }
}

/// Creates a `Service` from a function that returns responses immediately.
// FIXME: use async fn
pub fn service_fn_ok<F, Req, Res, E>(f: F) -> ServiceFnOk<F, Req, Res, E>
where
    F: FnMut(Req) -> Res,
{
    ServiceFnOk {
        f,
        _marker: PhantomData,
    }
}

#[derive(Debug)]
pub struct ServiceFnOk<F, Req, Res, E> {
    f: F,
    _marker: PhantomData<fn(Req) -> (Res, E)>,
}

impl<F, Req, Res, E> Service<Req> for ServiceFnOk<F, Req, Res, E>
where
    F: FnMut(Req) -> Res,
{
    type Response = Res;
    type Error = E;
    type Future = futures::future::FutureResult<Self::Response, Self::Error>;

    #[inline]
    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    #[inline]
    fn call(&mut self, request: Req) -> Self::Future {
        futures::future::ok((self.f)(request))
    }
}

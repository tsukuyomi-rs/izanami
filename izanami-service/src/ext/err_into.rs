#![allow(missing_docs)]

use {
    crate::Service,
    futures::{future, Future, Poll},
    std::marker::PhantomData,
};

#[derive(Debug)]
pub struct ErrInto<S, E> {
    pub(super) service: S,
    pub(super) _marker: PhantomData<fn() -> E>,
}

#[allow(clippy::type_complexity)]
impl<S, E, Req> Service<Req> for ErrInto<S, E>
where
    S: Service<Req>,
    S::Error: Into<E>,
{
    type Response = S::Response;
    type Error = E;
    type Future = future::MapErr<S::Future, fn(S::Error) -> E>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.service.poll_ready().map_err(Into::into)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        self.service.call(req).map_err(Into::into)
    }
}

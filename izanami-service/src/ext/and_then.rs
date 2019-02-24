#![allow(missing_docs)]

use {
    crate::Service,
    futures::{future, Future, IntoFuture, Poll},
};

#[derive(Debug)]
pub struct AndThen<S, F> {
    pub(super) service: S,
    pub(super) f: F,
}

impl<S, F, R, Req> Service<Req> for AndThen<S, F>
where
    S: Service<Req>,
    F: Fn(S::Response) -> R + Clone,
    R: IntoFuture<Error = S::Error>,
{
    type Response = R::Item;
    type Error = S::Error;
    type Future = future::AndThen<S::Future, R, F>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.service.poll_ready()
    }

    fn call(&mut self, req: Req) -> Self::Future {
        self.service.call(req).and_then(self.f.clone())
    }
}

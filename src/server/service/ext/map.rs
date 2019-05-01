#![allow(missing_docs)]

use {
    futures::{future, Future, Poll},
    tower_service::Service,
};

#[derive(Debug)]
pub struct Map<S, F> {
    pub(super) service: S,
    pub(super) f: F,
}

impl<S, F, Res, Req> Service<Req> for Map<S, F>
where
    S: Service<Req>,
    F: Fn(S::Response) -> Res + Clone,
{
    type Response = Res;
    type Error = S::Error;
    type Future = future::Map<S::Future, F>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.service.poll_ready()
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let f = self.f.clone();
        self.service.call(req).map(f)
    }
}

#![allow(missing_docs)]

use {
    futures::{future, Future, Poll},
    tower_service::Service,
};

#[derive(Debug)]
pub struct MapErr<S, F> {
    pub(super) service: S,
    pub(super) f: F,
}

impl<S, F, E, Req> Service<Req> for MapErr<S, F>
where
    S: Service<Req>,
    F: Fn(S::Error) -> E + Clone,
{
    type Response = S::Response;
    type Error = E;
    type Future = future::MapErr<S::Future, F>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.service.poll_ready().map_err(&self.f)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let f = self.f.clone();
        self.service.call(req).map_err(f)
    }
}

use {
    crate::Service,
    futures::{Async, Poll},
};

/// Create a Service that returns `()`.
pub fn unit() -> UnitService {
    UnitService(())
}

#[derive(Debug)]
pub struct UnitService(());

impl<T> Service<T> for UnitService {
    type Response = ();
    type Error = std::io::Error; // FIXME: use never_type
    type Future = futures::future::FutureResult<Self::Response, Self::Error>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, _: T) -> Self::Future {
        futures::future::ok(())
    }
}

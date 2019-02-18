use {
    crate::Service,
    futures::{Future, Poll},
};

/// A trait representing an asynchronous factory of `Service`s.
pub trait MakeService<Ctx, Request>: self::sealed::Sealed<Ctx, Request> {
    /// The response type returned by `Service`.
    type Response;

    /// The error type returned by `Service`.
    type Error;

    /// The type of services created by this factory.
    type Service: Service<Request, Response = Self::Response, Error = Self::Error>;

    /// The type of errors that occur while creating `Service`.
    type MakeError;

    /// The type of `Future` returned from `make_service`.
    type Future: Future<Item = Self::Service, Error = Self::MakeError>;

    #[doc(hidden)]
    fn poll_ready(&mut self) -> Poll<(), Self::MakeError>;

    /// Creates a `Future` that will return a value of `Service`.
    fn make_service(&mut self, ctx: Ctx) -> Self::Future;
}

impl<S, Ctx, Request> MakeService<Ctx, Request> for S
where
    S: Service<Ctx>,
    S::Response: Service<Request>,
{
    type Response = <S::Response as Service<Request>>::Response;
    type Error = <S::Response as Service<Request>>::Error;
    type Service = S::Response;
    type MakeError = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self) -> Poll<(), Self::MakeError> {
        Service::poll_ready(self)
    }

    fn make_service(&mut self, ctx: Ctx) -> Self::Future {
        Service::call(self, ctx)
    }
}

mod sealed {
    use super::*;

    pub trait Sealed<Ctx, Request> {}

    impl<S, Ctx, Request> Sealed<Ctx, Request> for S
    where
        S: Service<Ctx>,
        S::Response: Service<Request>,
    {
    }
}

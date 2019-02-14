//! Abstraction of HTTP services based on [`tower-service`].
//!
//! [`tower-service`]: https://crates.io/crates/tower-service

#![doc(html_root_url = "https://docs.rs/izanami-service/0.1.0-preview.1")]
#![deny(
    missing_docs,
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]

use futures::{Async, Future, IntoFuture, Poll};

#[doc(no_inline)]
pub use tower_service::Service;

/// Creates a `Service` from the specified closure.
pub fn service_fn<Request, R>(
    f: impl FnMut(Request) -> R,
) -> impl Service<
    Request, //
    Response = R::Item,
    Error = R::Error,
    Future = R::Future,
>
where
    R: IntoFuture,
{
    #[allow(missing_debug_implementations)]
    struct ServiceFn<F>(F);

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

    ServiceFn(f)
}

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

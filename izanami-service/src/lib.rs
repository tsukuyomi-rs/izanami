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

pub mod http;
mod util;

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

/// A trait representing a factory of `Service`s.
///
/// The signature of this trait imitates `tower_util::MakeService`,
/// but there are the following differences:
///
/// * This trait does not have the method `poll_ready` to check
///   if the factory is ready for creating a `Service`.
/// * The method `make_service` is *immutable*.
pub trait MakeService<Ctx, Request> {
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

    /// Creates a `Future` that will return a value of `Service`.
    fn make_service(&self, ctx: Ctx) -> Self::Future;
}

/// An *alias* of `MakeService` receiving the context value of `Ctx` as reference.
#[allow(missing_docs)]
pub trait MakeServiceRef<Ctx, Request> {
    type Response;
    type Error;
    type Service: Service<Request, Response = Self::Response, Error = Self::Error>;
    type MakeError;
    type Future: Future<Item = Self::Service, Error = Self::MakeError>;

    fn make_service_ref(&self, ctx: &Ctx) -> Self::Future;
}

impl<S, T, Req, Res, Err, Svc, MkErr, Fut> MakeServiceRef<T, Req> for S
where
    for<'a> S: MakeService<
        &'a T,
        Req,
        Response = Res,
        Error = Err,
        Service = Svc,
        MakeError = MkErr,
        Future = Fut,
    >,
    Svc: Service<Req, Response = Res, Error = Err>,
    Fut: Future<Item = Svc, Error = MkErr>,
{
    type Response = Res;
    type Error = Err;
    type Service = Svc;
    type MakeError = MkErr;
    type Future = Fut;

    #[inline]
    fn make_service_ref(&self, ctx: &T) -> Self::Future {
        MakeService::make_service(self, ctx)
    }
}

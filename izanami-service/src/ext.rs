//! Extension for `Service`s.

mod and_then;
mod err_into;
mod map;
mod map_err;

pub use self::{
    and_then::AndThen, //
    err_into::ErrInto,
    map::Map,
    map_err::MapErr,
};

use {
    crate::Service, //
    futures::{IntoFuture, Poll},
    std::marker::PhantomData,
};

/// A `Service` that fixes request type to the specified one.
#[derive(Debug)]
pub struct FixedService<S, Req>
where
    S: Service<Req>,
{
    inner: S,
    _marker: PhantomData<fn(Req)>,
}

impl<S, Req> FixedService<S, Req>
where
    S: Service<Req>,
{
    /// Modifies the inner service to map the response value into a different type using the specified function.
    pub fn map<F, Res>(self, f: F) -> FixedService<Map<S, F>, Req>
    where
        F: Fn(S::Response) -> Res + Clone,
    {
        FixedService {
            inner: Map {
                service: self.inner,
                f,
            },
            _marker: PhantomData,
        }
    }

    /// Modifies the inner service to map the error into a different type using the specified function.
    pub fn map_err<F, E>(self, f: F) -> FixedService<MapErr<S, F>, Req>
    where
        F: Fn(S::Error) -> E + Clone,
    {
        FixedService {
            inner: MapErr {
                service: self.inner,
                f,
            },
            _marker: PhantomData,
        }
    }

    /// Modifies the inner service to map the error value into the specified type.
    pub fn err_into<E>(self) -> FixedService<ErrInto<S, E>, Req>
    where
        S::Error: Into<E>,
    {
        FixedService {
            inner: ErrInto {
                service: self.inner,
                _marker: PhantomData,
            },
            _marker: PhantomData,
        }
    }

    /// Modifies the inner service to execute the future returned from the specified function on success.
    pub fn and_then<F, R>(self, f: F) -> FixedService<AndThen<S, F>, Req>
    where
        F: Fn(S::Response) -> R + Clone,
        R: IntoFuture<Error = S::Error>,
    {
        FixedService {
            inner: AndThen {
                service: self.inner,
                f,
            },
            _marker: PhantomData,
        }
    }

    /// Returns the inner `Service`.
    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S, Req> Service<Req> for FixedService<S, Req>
where
    S: Service<Req>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    #[inline]
    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.inner.poll_ready()
    }

    #[inline]
    fn call(&mut self, request: Req) -> Self::Future {
        self.inner.call(request)
    }
}

/// A set of extensions for `Service`s.
///
/// The implementation of this trait is provided for *all* types, but only
/// the types that has implementation of `Service<Request>` can use the provided
/// extensions from this trait.
pub trait ServiceExt {
    /// Fix the request type of this service to the specified one.
    ///
    /// At the same time, this method wraps the service with `FixedService`
    /// to provide some adapter methods.
    fn fixed_service<Req>(self) -> FixedService<Self, Req>
    where
        Self: Service<Req> + Sized,
    {
        FixedService {
            inner: self,
            _marker: PhantomData,
        }
    }
}

impl<S> ServiceExt for S {}

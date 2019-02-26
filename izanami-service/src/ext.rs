//! Extension for `Service`s.

mod and_then;
mod err_into;
mod map;
mod map_err;
mod with;

pub use self::{
    and_then::AndThen, //
    err_into::ErrInto,
    map::Map,
    map_err::MapErr,
    with::With,
};

use {
    crate::Service, //
    futures::{IntoFuture, Poll},
    std::marker::PhantomData,
};

/// A wraper for `Service` that adds adaptor methods.
#[derive(Debug)]
pub struct WithAdaptors<S, Req>
where
    S: Service<Req>,
{
    inner: S,
    _marker: PhantomData<fn(Req)>,
}

impl<S, Req> WithAdaptors<S, Req>
where
    S: Service<Req>,
{
    #[allow(missing_docs)]
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            _marker: PhantomData,
        }
    }

    /// Modifies the inner service to map the response value into a different type using the specified function.
    pub fn map<F, Res>(self, f: F) -> WithAdaptors<Map<S, F>, Req>
    where
        F: Fn(S::Response) -> Res + Clone,
    {
        WithAdaptors::new(Map {
            service: self.inner,
            f,
        })
    }

    /// Modifies the inner service to map the error into a different type using the specified function.
    pub fn map_err<F, E>(self, f: F) -> WithAdaptors<MapErr<S, F>, Req>
    where
        F: Fn(S::Error) -> E + Clone,
    {
        WithAdaptors::new(MapErr {
            service: self.inner,
            f,
        })
    }

    /// Modifies the inner service to map the error value into the specified type.
    pub fn err_into<E>(self) -> WithAdaptors<ErrInto<S, E>, Req>
    where
        S::Error: Into<E>,
    {
        WithAdaptors::new(ErrInto {
            service: self.inner,
            _marker: PhantomData,
        })
    }

    /// Modifies the inner service to execute the future returned from the specified function on success.
    pub fn and_then<F, R>(self, f: F) -> WithAdaptors<AndThen<S, F>, Req>
    where
        F: Fn(S::Response) -> R + Clone,
        R: IntoFuture<Error = S::Error>,
    {
        WithAdaptors::new(AndThen {
            service: self.inner,
            f,
        })
    }

    /// Combines the inner service with the the specified one.
    ///
    /// The specified `Service` has the same request type as the inner service.
    pub fn with<S2>(self, service: S2) -> WithAdaptors<With<S, S2>, Req>
    where
        S2: Service<Req, Error = S::Error>,
        Req: Clone,
    {
        WithAdaptors::new(With {
            s1: self.inner,
            s2: service,
        })
    }

    /// Returns the inner `Service`.
    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S, Req> Service<Req> for WithAdaptors<S, Req>
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
    /// Wraps itself into `WithAdaptors` to provide the adaptor methods.
    fn with_adaptors<Req>(self) -> WithAdaptors<Self, Req>
    where
        Self: Service<Req> + Sized,
    {
        WithAdaptors::new(self)
    }
}

impl<S> ServiceExt for S {}

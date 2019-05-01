//! Extension for `Service`s.

mod and_then;
mod err_into;
mod join;
mod map;
mod map_err;

pub use self::{
    and_then::AndThen, //
    err_into::ErrInto,
    join::Join,
    map::Map,
    map_err::MapErr,
};

use {
    futures::IntoFuture,
    std::marker::PhantomData,
    tower_service::Service, //
};

/// A set of extensions for `Service`s.
pub trait ServiceExt<Req>: Service<Req> + Sized {
    /// Maps the response value returned from this service into a different type using the specified function.
    fn service_map<F, Res>(self, f: F) -> Map<Self, F>
    where
        F: Fn(Self::Response) -> Res + Clone,
    {
        Map { service: self, f }
    }

    /// Maps the error value produced by this service into a different type using the specified function.
    fn service_map_err<F, E>(self, f: F) -> MapErr<Self, F>
    where
        F: Fn(Self::Error) -> E + Clone,
    {
        MapErr { service: self, f }
    }

    /// Converts the error value produced by this service into a different type.
    fn service_err_into<E>(self) -> ErrInto<Self, E>
    where
        Self::Error: Into<E>,
    {
        ErrInto {
            service: self,
            _marker: PhantomData,
        }
    }

    /// Executes the future returned from the specified function when this service returns a response.
    fn service_and_then<F, R>(self, f: F) -> AndThen<Self, F>
    where
        F: Fn(Self::Response) -> R + Clone,
        R: IntoFuture<Error = Self::Error>,
    {
        AndThen { service: self, f }
    }

    /// Combines this service with the the specified one.
    ///
    /// The specified `Service` has the same request type as this service.
    fn service_join<S>(self, service: S) -> Join<Self, S>
    where
        S: Service<Req, Error = Self::Error>,
        Req: Clone,
    {
        Join {
            s1: self,
            s2: service,
        }
    }
}

impl<S, Req> ServiceExt<Req> for S where S: Service<Req> {}

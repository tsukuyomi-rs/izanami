use {
    crate::Service,
    futures::{Future, Poll},
    izanami_util::*,
};

/// The kinds of preparation state returned from `StreamService`.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum StreamServiceState {
    /// The service is ready to call.
    Ready,

    /// The service was shutdown in a normal procedure.
    Completed,
}

/// An extension of `Service` which provides the ability
/// to return the preparing state.
///
/// The signature of this trait is almost the same as `Service`,
/// except that the return type of `poll_ready`.
pub trait StreamService<T> {
    /// The response type of produced future.
    type Response;

    /// The error type of produced future.
    type Error;

    /// The type of future produced by this stream.
    type Future: Future<Item = Self::Response, Error = Self::Error>;

    /// Returns whether to ready the service is able to process requests.
    ///
    /// Unlike `Service::poll_ready`, this method may return a `Completed`
    /// that indicates the service has already been terminated in a normal
    /// procedure. these situation may occur when the inner `Stream` returned
    /// an `Ok(Async::Ready(None))`.
    fn poll_ready(&mut self) -> Poll<StreamServiceState, Self::Error>;

    /// Process the request and return its response asynchronously.
    ///
    /// This method may cause a panic when the service has already been terminated.
    fn call(&mut self, target: T) -> Self::Future;
}

impl<S, T> StreamService<T> for S
where
    S: Service<T>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self) -> Poll<StreamServiceState, Self::Error> {
        Service::poll_ready(self).map_async(|()| StreamServiceState::Ready)
    }

    #[inline]
    fn call(&mut self, target: T) -> Self::Future {
        Service::call(self, target)
    }
}

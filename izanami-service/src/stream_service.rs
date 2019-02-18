use {
    crate::Service,
    futures::{Async, Future, Poll},
};

/// Asynchronous stream that produces a sequence of futures.
///
/// The role of this trait is similar to [`Stream`], except that
/// the value produced by this stream is [`Future`].
///
/// [`Stream`]: https://docs.rs/futures/0.1/futures/stream/trait.Stream.html
/// [`Future`]: https://docs.rs/futures/0.1/futures/future/trait.Future.html
#[allow(missing_docs)]
pub trait StreamService {
    /// The response type of produced future.
    type Response;

    /// The error type of produced future.
    type Error;

    /// The type of future produced by this stream.
    type Future: Future<Item = Self::Response, Error = Self::Error>;

    /// Attempts to pull out a future from this stream.
    fn poll_next_service(&mut self) -> Poll<Option<Self::Future>, Self::Error>;
}

impl<S> StreamService for S
where
    S: Service<()>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    #[inline]
    fn poll_next_service(&mut self) -> Poll<Option<Self::Future>, Self::Error> {
        futures::try_ready!(Service::poll_ready(self));
        Ok(Async::Ready(Some(Service::call(self, ()))))
    }
}

//! Miscellaneous utilities for interaction with the underlying runtime.

#[doc(no_inline)]
pub use tokio_threadpool::{
    blocking as poll_blocking, //
    BlockingError,
};

use futures::Future;

/// Creates a `Future` to enter the specified blocking section of code.
///
/// The future genereted by this function internally calls the Tokio's blocking API,
/// and then enters a blocking section after other tasks are moved to another thread.
/// See [the documentation of `tokio_threadpool::blocking`][blocking] for details.
///
/// [blocking]: https://docs.rs/tokio-threadpool/0.1/tokio_threadpool/fn.blocking.html
pub fn blocking_section<F, T>(op: F) -> BlockingSection<F>
where
    F: FnOnce() -> T,
{
    BlockingSection { op: Some(op) }
}

/// The future that enters a blocking section of code.
#[derive(Debug)]
pub struct BlockingSection<F> {
    op: Option<F>,
}

impl<F, T> Future for BlockingSection<F>
where
    F: FnOnce() -> T,
{
    type Item = T;
    type Error = BlockingError;

    #[inline]
    fn poll(&mut self) -> futures::Poll<Self::Item, Self::Error> {
        poll_blocking(|| {
            let op = self.op.take().expect("The future has already been polled");
            op()
        })
    }
}

//! Miscellaneous primitives used within izanami.

use futures::Poll;

pub trait MapAsyncExt<T, E> {
    fn map_async<U>(self, op: impl FnOnce(T) -> U) -> Poll<U, E>;
}

impl<T, E> MapAsyncExt<T, E> for Poll<T, E> {
    fn map_async<U>(self, op: impl FnOnce(T) -> U) -> Poll<U, E> {
        self.map(|x| x.map(op))
    }
}

pub trait MapAsyncOptExt<T, E> {
    fn map_async_opt<U>(self, op: impl FnOnce(T) -> U) -> Poll<Option<U>, E>;
}

impl<T, E> MapAsyncOptExt<T, E> for Poll<Option<T>, E> {
    fn map_async_opt<U>(self, op: impl FnOnce(T) -> U) -> Poll<Option<U>, E> {
        self.map(|x| x.map(|opt| opt.map(op)))
    }
}

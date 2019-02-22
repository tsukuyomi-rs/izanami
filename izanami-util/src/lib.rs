//! Miscellaneous primitives used within izanami.

#![doc(html_root_url = "https://docs.rs/izanami-util/0.1.0-preview.1")]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]

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

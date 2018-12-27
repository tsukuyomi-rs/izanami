//!

#![allow(missing_docs)]

use {
    bytes::Buf,
    futures::{Future, Poll},
};

/// A trait which abstracts an asynchronous stream of bytes.
///
/// The purpose of this trait is to imitate the trait defined in
/// (unreleased) `tokio-buf`, and it will be replaced by it in the future.
pub trait BufStream {
    type Item: Buf;
    type Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error>;
}

pub trait Upgradable {
    type Upgraded;
    type Error;
    type Future: Future<Item = Self::Upgraded, Error = Self::Error>;

    fn upgrade(self) -> Self::Future;
}

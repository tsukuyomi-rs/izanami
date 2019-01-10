//! Components that abstracts HTTP upgrades.

use {
    futures::Future,
    http::Request,
    tokio_io::{AsyncRead, AsyncWrite},
};

/// A trait abstracting an HTTP upgrade.
///
/// Typically, this trait is implemented by the types that represents
/// the request body (e.g. `hyper::Body`).
pub trait OnUpgrade {
    /// The type of upgraded I/O.
    type Upgraded: AsyncRead + AsyncWrite;

    /// The type of error that will be returned from `Future`.
    type Error;

    /// The type of associated `Future` that will return an upgraded I/O.
    type Future: Future<Item = Self::Upgraded, Error = Self::Error>;

    /// Converts itself into a `Future` that will return an upgraded I/O.
    fn on_upgrade(self) -> Self::Future;
}

impl<T> OnUpgrade for Request<T>
where
    T: OnUpgrade,
{
    type Upgraded = T::Upgraded;
    type Error = T::Error;
    type Future = T::Future;

    fn on_upgrade(self) -> Self::Future {
        self.into_body().on_upgrade()
    }
}

#[cfg(feature = "hyper")]
mod impl_hyper {
    use super::*;

    impl OnUpgrade for hyper::Body {
        type Upgraded = hyper::upgrade::Upgraded;
        type Error = hyper::Error;
        type Future = hyper::upgrade::OnUpgrade;

        fn on_upgrade(self) -> Self::Future {
            self.on_upgrade()
        }
    }
}

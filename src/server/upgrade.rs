//! HTTP upgrade abstraction.

use {super::Connection, tokio_buf::BufStream};

/// A trait that abstracts the behavior after upgrading request to another protocol.
pub trait HttpUpgrade<I> {
    type Upgraded: Connection<Error = Self::UpgradeError>;
    type UpgradeError;

    fn upgrade(self, io: I) -> Result<Self::Upgraded, I>;
}

impl<Bd: BufStream, I> HttpUpgrade<I> for Bd {
    type Upgraded = futures::future::Empty<(), Self::UpgradeError>;
    type UpgradeError = std::convert::Infallible;

    fn upgrade(self, io: I) -> Result<Self::Upgraded, I> {
        Err(io)
    }
}

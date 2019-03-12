//! HTTP/1 upgrade.

#![allow(missing_docs)]

use {futures::Poll, std::fmt, tokio_buf::BufStream};

pub trait HttpUpgrade<I> {
    /// The type of asynchronous process that drives upgraded protocol.
    type Upgraded: Upgraded<Error = Self::Error>;

    /// The error type that will be returned from the upgraded stream.
    type Error;

    /// Upgrades the specified stream to another protocol.
    ///
    /// The implementation of this method may return a `Err(stream)`
    /// when the value cannot upgrade the stream.
    fn upgrade(self, stream: I) -> Result<Self::Upgraded, I>;
}

impl<Bd, I> HttpUpgrade<I> for Bd
where
    Bd: BufStream,
{
    type Upgraded = Never;
    type Error = Never;

    fn upgrade(self, stream: I) -> Result<Self::Upgraded, I> {
        Err(stream)
    }
}

pub trait Upgraded {
    type Error;

    fn poll_done(&mut self) -> Poll<(), Self::Error>;

    fn graceful_shutdown(&mut self);
}

pub enum Never {}

impl fmt::Debug for Never {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

impl fmt::Display for Never {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

impl std::error::Error for Never {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {}
    }
}

impl Upgraded for Never {
    type Error = Never;

    fn poll_done(&mut self) -> Poll<(), Self::Error> {
        match *self {}
    }

    fn graceful_shutdown(&mut self) {
        match *self {}
    }
}

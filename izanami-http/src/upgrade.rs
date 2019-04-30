//! HTTP upgrade abstraction.

use {
    crate::body::{Body, HttpBody}, //
    futures::{Async, Poll},
    std::{any::TypeId, io},
    tokio_buf::{BufStream, SizeHint},
    tokio_io::{AsyncRead, AsyncWrite},
};

trait Io: AsyncRead + AsyncWrite + 'static {
    fn __type_id__(&self) -> TypeId {
        TypeId::of::<Self>()
    }
}

impl<I: AsyncRead + AsyncWrite + 'static> Io for I {}

#[allow(missing_docs)]
#[allow(missing_debug_implementations)]
pub struct Upgraded<'a> {
    io: &'a mut dyn Io,
}

#[allow(missing_docs)]
impl<'a> Upgraded<'a> {
    pub fn downcast_ref<T>(&self) -> Option<&T>
    where
        T: AsyncRead + AsyncWrite + 'static,
    {
        None
    }

    pub fn downcast_mut<T>(&mut self) -> Option<&mut T>
    where
        T: AsyncRead + AsyncWrite + 'static,
    {
        None
    }
}

impl<'a, I> From<&'a mut I> for Upgraded<'a>
where
    I: AsyncRead + AsyncWrite + 'static,
{
    fn from(io: &'a mut I) -> Self {
        Self { io }
    }
}

impl<'a> io::Read for Upgraded<'a> {
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        self.io.read(dst)
    }
}

impl<'a> io::Write for Upgraded<'a> {
    fn write(&mut self, src: &[u8]) -> io::Result<usize> {
        self.io.write(src)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.io.flush()
    }
}

impl<'a> AsyncRead for Upgraded<'a> {
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.io.prepare_uninitialized_buffer(buf)
    }
}

impl<'a> AsyncWrite for Upgraded<'a> {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.io.shutdown()
    }
}

/// A trait that abstracts the behavior after upgrading request to another protocol.
pub trait HttpUpgrade {
    /// The error type that will be returned from `poll_upgraded`.
    type UpgradeError;

    /// Polls the state of this context using the provided I/O.
    fn poll_upgraded(&mut self, io: &mut Upgraded<'_>) -> Poll<(), Self::UpgradeError>;

    /// Notifies to transition to the shutdown state.
    fn notify_shutdown(&mut self);
}

impl<Bd: BufStream> HttpUpgrade for Bd {
    type UpgradeError = std::convert::Infallible;

    fn poll_upgraded(&mut self, _: &mut Upgraded<'_>) -> Poll<(), Self::UpgradeError> {
        Ok(Async::Ready(()))
    }

    fn notify_shutdown(&mut self) {}
}

impl HttpUpgrade for Body {
    type UpgradeError = std::convert::Infallible;

    fn poll_upgraded(&mut self, _: &mut Upgraded<'_>) -> Poll<(), Self::UpgradeError> {
        Ok(Async::Ready(()))
    }

    fn notify_shutdown(&mut self) {}
}

/// A wrapper for `HttpBody` for adding implementation of `HttpUpgrade`.
#[derive(Debug)]
pub struct NoUpgrade<T: HttpBody>(pub T);

impl<T> HttpBody for NoUpgrade<T>
where
    T: HttpBody,
{
    type Data = T::Data;
    type Error = T::Error;

    fn poll_data(&mut self) -> Poll<Option<Self::Data>, Self::Error> {
        self.0.poll_data()
    }

    fn size_hint(&self) -> SizeHint {
        self.0.size_hint()
    }

    fn poll_trailers(&mut self) -> Poll<Option<http::HeaderMap>, Self::Error> {
        self.0.poll_trailers()
    }

    fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }

    fn content_length(&self) -> Option<u64> {
        self.0.content_length()
    }
}

impl<T: HttpBody> HttpUpgrade for NoUpgrade<T> {
    type UpgradeError = std::convert::Infallible;

    fn poll_upgraded(&mut self, _: &mut Upgraded<'_>) -> Poll<(), Self::UpgradeError> {
        Ok(Async::Ready(()))
    }

    fn notify_shutdown(&mut self) {}
}

/// An `HttpUpgrade` that contains either of an `HttpBody` or `HttpUpgrade`.
#[derive(Debug)]
pub enum MaybeUpgrade<T: HttpBody, U> {
    /// The body will be sent to the client without upgrading.
    Data(T),

    /// The body will be upgraded.
    Upgrade(U),
}

impl<T, U> HttpBody for MaybeUpgrade<T, U>
where
    T: HttpBody,
{
    type Data = T::Data;
    type Error = T::Error;

    fn poll_data(&mut self) -> Poll<Option<<Self as HttpBody>::Data>, Self::Error> {
        match self {
            MaybeUpgrade::Data(ref mut data) => data.poll_data(),
            MaybeUpgrade::Upgrade(..) => Ok(Async::Ready(None)),
        }
    }

    fn size_hint(&self) -> SizeHint {
        match self {
            MaybeUpgrade::Data(ref data) => data.size_hint(),
            MaybeUpgrade::Upgrade(..) => SizeHint::new(),
        }
    }

    fn poll_trailers(&mut self) -> Poll<Option<http::HeaderMap>, Self::Error> {
        match self {
            MaybeUpgrade::Data(ref mut data) => data.poll_trailers(),
            MaybeUpgrade::Upgrade(..) => Ok(Async::Ready(None)),
        }
    }

    fn is_end_stream(&self) -> bool {
        match self {
            MaybeUpgrade::Data(ref data) => data.is_end_stream(),
            MaybeUpgrade::Upgrade(..) => true,
        }
    }

    fn content_length(&self) -> Option<u64> {
        match self {
            MaybeUpgrade::Data(ref data) => data.content_length(),
            MaybeUpgrade::Upgrade(..) => None,
        }
    }
}

impl<T, U> HttpUpgrade for MaybeUpgrade<T, U>
where
    T: HttpBody,
    U: HttpUpgrade,
{
    type UpgradeError = U::UpgradeError;

    fn poll_upgraded(&mut self, io: &mut Upgraded<'_>) -> Poll<(), Self::UpgradeError> {
        match self {
            MaybeUpgrade::Upgrade(cx) => cx.poll_upgraded(io),
            MaybeUpgrade::Data(..) => Ok(Async::Ready(())),
        }
    }

    fn notify_shutdown(&mut self) {
        match self {
            MaybeUpgrade::Upgrade(cx) => cx.notify_shutdown(),
            MaybeUpgrade::Data(..) => (),
        }
    }
}

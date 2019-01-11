//! Components that abstracts HTTP upgrades.

use {
    futures::{Future, Poll},
    http::Request,
    std::any::TypeId,
    tokio_io::{AsyncRead, AsyncWrite},
};

/// A trait that represents an upgraded I/O.
pub trait Upgraded: AsyncRead + AsyncWrite {
    // not a public API.
    #[doc(hidden)]
    fn __private_type_id__(&self) -> TypeId
    where
        Self: 'static,
    {
        TypeId::of::<Self>()
    }
}

impl<T> Upgraded for T where T: AsyncRead + AsyncWrite {}

impl dyn Upgraded + 'static {
    /// Returns whether the boxed type is the same as `T` or not.
    pub fn is<T>(&self) -> bool
    where
        T: Upgraded + 'static,
    {
        self.__private_type_id__() == TypeId::of::<T>()
    }

    /// Attempts to downcast the object to a concrete type as a reference.
    pub fn downcast_ref<T>(&self) -> Option<&T>
    where
        T: Upgraded + 'static,
    {
        if self.is::<T>() {
            Some(unsafe { &*(self as *const Self as *const T) })
        } else {
            None
        }
    }

    /// Attempts to downcast the object to a concrete type as a mutable reference.
    pub fn downcast_mut<T>(&mut self) -> Option<&mut T>
    where
        T: Upgraded + 'static,
    {
        if self.is::<T>() {
            Some(unsafe { &mut *(self as *mut Self as *mut T) })
        } else {
            None
        }
    }

    /// Attempts to downcast the object to a concrete type.
    pub fn downcast<T>(self: Box<Self>) -> Result<Box<T>, Box<Self>>
    where
        T: Upgraded + 'static,
    {
        if self.is::<T>() {
            Ok(unsafe { Box::from_raw(Box::into_raw(self) as *mut T) })
        } else {
            Err(self)
        }
    }
}

impl dyn Upgraded + Send + 'static {
    /// Returns whether the boxed type is the same as `T` or not.
    pub fn is<T>(&self) -> bool
    where
        T: Upgraded + Send + 'static,
    {
        self.__private_type_id__() == TypeId::of::<T>()
    }

    /// Attempts to downcast the object to a concrete type as a reference.
    pub fn downcast_ref<T>(&self) -> Option<&T>
    where
        T: Upgraded + Send + 'static,
    {
        if self.is::<T>() {
            Some(unsafe { &*(self as *const Self as *const T) })
        } else {
            None
        }
    }

    /// Attempts to downcast the object to a concrete type as a mutable reference.
    pub fn downcast_mut<T>(&mut self) -> Option<&mut T>
    where
        T: Upgraded + Send + 'static,
    {
        if self.is::<T>() {
            Some(unsafe { &mut *(self as *mut Self as *mut T) })
        } else {
            None
        }
    }

    /// Attempts to downcast the object to a concrete type.
    pub fn downcast<T>(self: Box<Self>) -> Result<Box<T>, Box<Self>>
    where
        T: Upgraded + Send + 'static,
    {
        if self.is::<T>() {
            Ok(unsafe { Box::from_raw(Box::into_raw(self) as *mut T) })
        } else {
            Err(self)
        }
    }
}

/// A trait that abstracts the HTTP upgrade.
///
/// Typically, this trait is implemented by the types that represents
/// the request body.
pub trait Upgrade {
    /// The type of upgraded I/O.
    type Upgraded: Upgraded;

    /// The type of error that will be returned from `Future`.
    type Error;

    /// Checks if the upgraded I/O is ready, and returns its value if possible.
    fn poll_upgrade(&mut self) -> Poll<Self::Upgraded, Self::Error>;

    /// Converts itself into a `Future` that will return an upgraded I/O.
    fn on_upgrade(self) -> OnUpgrade<Self>
    where
        Self: Sized,
    {
        OnUpgrade(self)
    }
}

/// A `Future` that checks if the upgraded I/O is ready.
#[derive(Debug)]
pub struct OnUpgrade<T>(T);

impl<T> Future for OnUpgrade<T>
where
    T: Upgrade,
{
    type Item = T::Upgraded;
    type Error = T::Error;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.0.poll_upgrade()
    }
}

impl<T> Upgrade for Request<T>
where
    T: Upgrade,
{
    type Upgraded = T::Upgraded;
    type Error = T::Error;

    fn poll_upgrade(&mut self) -> Poll<Self::Upgraded, Self::Error> {
        self.body_mut().poll_upgrade()
    }
}

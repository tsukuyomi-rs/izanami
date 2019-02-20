//! HTTP/1 upgrade.

use {
    futures::Future,
    http::Request,
    std::any::TypeId,
    tokio_io::{AsyncRead, AsyncWrite},
};

/// A trait that abstracts the HTTP upgrade.
///
/// Typically, this trait is implemented by the types that represents
/// the request body.
pub trait Upgrade {
    /// An upgraded I/O.
    type Upgraded: Upgraded;

    /// The error type on upgrade.
    type Error;

    /// The future that returns an upgraded I/O from HTTP.
    type Future: Future<Item = Self::Upgraded, Error = Self::Error>;

    /// Consume itself and create a `Future` that returns the upgraded I/O.
    ///
    /// Typically, the future returned from this method will not be ready
    /// until the server completes to send the handshake response to client.
    /// DO NOT poll this value on the same task as the future returning the
    /// response.
    fn on_upgrade(self) -> Self::Future;
}

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

impl<T> Upgrade for Request<T>
where
    T: Upgrade,
{
    type Upgraded = T::Upgraded;
    type Error = T::Error;
    type Future = T::Future;

    fn on_upgrade(self) -> Self::Future {
        self.into_body().on_upgrade()
    }
}

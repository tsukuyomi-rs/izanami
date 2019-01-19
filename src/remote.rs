use std::net::SocketAddr;

#[cfg(unix)]
mod unix {
    pub(super) use std::os::unix::net::SocketAddr;
}

#[derive(Debug, Clone)]
enum RemoteAddrKind {
    Tcp(SocketAddr),
    #[cfg(unix)]
    Unix(unix::SocketAddr),
    Unknown,
}

/// A type representing the address of the peer.
///
/// The value of this type is typically contained into the extension map
/// of `Request` before calling the service.
#[derive(Debug, Clone)]
pub struct RemoteAddr(RemoteAddrKind);

impl RemoteAddr {
    /// Create a `RemoteAddr` indicating that the remote address cannot be acquired.
    #[inline]
    pub fn unknown() -> Self {
        Self(RemoteAddrKind::Unknown)
    }

    /// Create a `RemoteAddr` from the specified value of [`SocketAddr`].
    ///
    /// [`SocketAddr`]: https://doc.rust-lang.org/std/net/enum.SocketAddr.html
    #[inline]
    pub fn tcp(addr: SocketAddr) -> Self {
        Self(RemoteAddrKind::Tcp(addr))
    }

    /// Create a `RemoteAddr` from the specified value of [`SocketAddr`].
    ///
    /// This function is available only on Unix platform.
    ///
    /// [`SocketAddr`]: https://doc.rust-lang.org/std/os/unix/net/struct.SocketAddr.html
    #[cfg(unix)]
    #[inline]
    pub fn unix(addr: unix::SocketAddr) -> Self {
        Self(RemoteAddrKind::Unix(addr))
    }

    /// Returns whether this address is unknown or not.
    #[inline]
    pub fn is_unknown(&self) -> bool {
        match self.0 {
            RemoteAddrKind::Unknown => true,
            _ => false,
        }
    }

    /// Returns whether this address originates from TCP or not.
    #[inline]
    pub fn is_tcp(&self) -> bool {
        match self.0 {
            RemoteAddrKind::Tcp(..) => true,
            _ => false,
        }
    }

    /// Returns whether this address originates from the Unix domain socket or not.
    ///
    /// This method is available only on Unix platform.
    #[cfg(unix)]
    #[inline]
    pub fn is_unix(&self) -> bool {
        match self.0 {
            RemoteAddrKind::Unix(..) => true,
            _ => false,
        }
    }

    /// Returns the representation as a [`SocketAddr`], if possible.
    ///
    /// [`SocketAddr`]: https://doc.rust-lang.org/std/net/enum.SocketAddr.html
    #[inline]
    pub fn as_tcp(&self) -> Option<&SocketAddr> {
        match &self.0 {
            RemoteAddrKind::Tcp(addr) => Some(addr),
            _ => None,
        }
    }

    /// Returns the representation as a [`SocketAddr`], if possible.
    ///
    /// [`SocketAddr`]: https://doc.rust-lang.org/std/os/unix/net/struct.SocketAddr.html
    #[cfg(unix)]
    #[inline]
    pub fn as_unix(&self) -> Option<&unix::SocketAddr> {
        match &self.0 {
            RemoteAddrKind::Unix(addr) => Some(addr),
            _ => None,
        }
    }
}

impl From<SocketAddr> for RemoteAddr {
    #[inline]
    fn from(addr: SocketAddr) -> Self {
        Self::tcp(addr)
    }
}

#[cfg(unix)]
impl From<unix::SocketAddr> for RemoteAddr {
    #[inline]
    fn from(addr: unix::SocketAddr) -> Self {
        Self::unix(addr)
    }
}

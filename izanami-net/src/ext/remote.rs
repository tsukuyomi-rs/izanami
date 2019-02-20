use std::net::SocketAddr;

#[cfg(unix)]
mod unix {
    pub(super) use std::os::unix::net::SocketAddr;
}

/// The value of remote address obtained from the transport.
///
/// Note that the value contained this type may different from
/// the actual client's address if the server is behind proxy.
#[derive(Debug, Clone)]
pub enum RemoteAddr {
    /// The peer's address associated with a TCP stream.
    Tcp(SocketAddr),

    /// The peer's address associated with a Unix socket.
    #[cfg(unix)]
    Unix(unix::SocketAddr),

    /// The arbitrary value with opaque type.
    Opaque(Vec<u8>),

    #[doc(hidden)]
    __NoExhausive(()),
}

impl RemoteAddr {
    /// Returns whether this address originates from TCP or not.
    #[inline]
    pub fn is_tcp(&self) -> bool {
        match self {
            RemoteAddr::Tcp(..) => true,
            _ => false,
        }
    }

    /// Returns whether this address originates from the Unix domain socket or not.
    ///
    /// This method is available only on Unix platform.
    #[cfg(unix)]
    #[inline]
    pub fn is_unix(&self) -> bool {
        match self {
            RemoteAddr::Unix(..) => true,
            _ => false,
        }
    }
}

impl From<SocketAddr> for RemoteAddr {
    #[inline]
    fn from(addr: SocketAddr) -> Self {
        RemoteAddr::Tcp(addr)
    }
}

#[cfg(unix)]
impl From<unix::SocketAddr> for RemoteAddr {
    #[inline]
    fn from(addr: unix::SocketAddr) -> Self {
        RemoteAddr::Unix(addr)
    }
}

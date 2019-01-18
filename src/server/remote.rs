use std::net::SocketAddr;

#[cfg(unix)]
mod unix {
    pub(super) use std::os::unix::net::SocketAddr;
}

#[cfg(not(unix))]
mod unix {
    use std::fmt;

    pub enum SocketAddr {}

    impl Clone for SocketAddr {
        fn clone(&self) -> Self {
            match *self {}
        }
    }

    impl fmt::Debug for SocketAddr {
        fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
            match *self {}
        }
    }

    impl fmt::Display for SocketAddr {
        fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
            match *self {}
        }
    }
}

#[derive(Debug, Clone)]
enum RemoteAddrKind {
    Tcp(SocketAddr),
    Uds(unix::SocketAddr),
    Unknown,
}

/// A type representing the address of the peer.
///
/// The value of this type is typically contained into the extension map
/// of `Request` before calling the service.
#[derive(Debug, Clone)]
pub struct RemoteAddr(RemoteAddrKind);

impl RemoteAddr {
    pub fn unknown() -> Self {
        Self(RemoteAddrKind::Unknown)
    }

    pub fn is_tcp(&self) -> bool {
        match self.0 {
            RemoteAddrKind::Tcp(..) => true,
            _ => false,
        }
    }

    #[cfg(unix)]
    pub fn is_uds(&self) -> bool {
        match self.0 {
            RemoteAddrKind::Uds(..) => true,
            _ => false,
        }
    }

    pub fn is_unknown(&self) -> bool {
        match self.0 {
            RemoteAddrKind::Unknown => true,
            _ => false,
        }
    }

    pub fn as_tcp(&self) -> Option<&SocketAddr> {
        match &self.0 {
            RemoteAddrKind::Tcp(addr) => Some(addr),
            _ => None,
        }
    }

    pub fn as_uds(&self) -> Option<&unix::SocketAddr> {
        match &self.0 {
            RemoteAddrKind::Uds(addr) => Some(addr),
            _ => None,
        }
    }
}

impl From<SocketAddr> for RemoteAddr {
    fn from(addr: SocketAddr) -> Self {
        Self(RemoteAddrKind::Tcp(addr))
    }
}

#[cfg(unix)]
impl From<unix::SocketAddr> for RemoteAddr {
    fn from(addr: unix::SocketAddr) -> Self {
        Self(RemoteAddrKind::Uds(addr))
    }
}

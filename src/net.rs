//! Abstraction around low-level network I/O.

use {
    futures::Poll,
    izanami_util::RemoteAddr,
    std::io,
    tokio::io::{AsyncRead, AsyncWrite},
};

/// A trait representing the conversion to an I/O object bound to the specified address.
pub trait Bind {
    /// The type of listener returned from `bind`.
    type Listener: Listener;

    /// Create an I/O object bound to the specified address.
    fn bind(self) -> crate::Result<Self::Listener>;
}

impl<T: Listener> Bind for T {
    type Listener = Self;

    #[inline]
    fn bind(self) -> crate::Result<Self::Listener> {
        Ok(self)
    }
}

/// A trait abstracting I/O objects that listens for incoming connections.
pub trait Listener {
    /// The type of established connection returned from `Incoming`.
    type Conn: AsyncRead + AsyncWrite;

    /// Acquires a connection to the peer.
    fn poll_incoming(&mut self) -> Poll<(Self::Conn, RemoteAddr), io::Error>;
}

pub mod tcp {
    use {
        super::{sleep_on_errors::SleepOnErrors, *},
        crate::util::MapAsyncExt,
        std::{net::SocketAddr, time::Duration},
        tokio::net::{TcpListener, TcpStream},
    };

    impl Listener for TcpListener {
        type Conn = TcpStream;

        #[inline]
        fn poll_incoming(&mut self) -> Poll<(Self::Conn, RemoteAddr), io::Error> {
            self.poll_accept()
                .map_async(|(conn, addr)| (conn, addr.into()))
        }
    }

    /// A TCP transport that allows to sleep on accept errors.
    #[derive(Debug)]
    pub struct AddrIncoming {
        listener: SleepOnErrors<TcpListener>,
        local_addr: SocketAddr,
        tcp_keepalive_timeout: Option<Duration>,
        tcp_nodelay: bool,
    }

    impl AddrIncoming {
        pub(crate) fn bind(addr: &SocketAddr) -> io::Result<Self> {
            let listener = TcpListener::bind(addr)?;
            let local_addr = listener.local_addr()?;
            Ok(Self {
                listener: SleepOnErrors::new(listener),
                local_addr,
                tcp_keepalive_timeout: None,
                tcp_nodelay: false,
            })
        }

        #[inline]
        pub fn local_addr(&self) -> SocketAddr {
            self.local_addr
        }

        #[inline]
        pub fn set_keepalive_timeout(&mut self, timeout: Option<Duration>) -> &mut Self {
            self.tcp_keepalive_timeout = timeout;
            self
        }

        #[inline]
        pub fn set_nodelay(&mut self, enabled: bool) -> &mut Self {
            self.tcp_nodelay = enabled;
            self
        }

        #[inline]
        pub fn set_sleep_on_errors(&mut self, duration: Option<Duration>) -> &mut Self {
            self.listener.set_sleep_on_errors(duration);
            self
        }
    }

    impl Listener for AddrIncoming {
        type Conn = TcpStream;

        #[inline]
        fn poll_incoming(&mut self) -> Poll<(Self::Conn, RemoteAddr), io::Error> {
            self.listener
                .poll_incoming() //
                .map_async(|(stream, addr)| {
                    if let Some(timeout) = self.tcp_keepalive_timeout {
                        if let Err(e) = stream.set_keepalive(Some(timeout)) {
                            log::trace!("error trying to set TCP keepalive: {}", e);
                        }
                    }
                    if let Err(e) = stream.set_nodelay(self.tcp_nodelay) {
                        log::trace!("error trying to set TCP nodelay: {}", e);
                    }
                    (stream, addr)
                })
        }
    }

    impl Bind for SocketAddr {
        type Listener = AddrIncoming;

        fn bind(self) -> crate::Result<Self::Listener> {
            (&self).bind()
        }
    }

    impl<'a> Bind for &'a SocketAddr {
        type Listener = AddrIncoming;

        fn bind(self) -> crate::Result<Self::Listener> {
            AddrIncoming::bind(self).map_err(Into::into)
        }
    }

    impl<'a> Bind for &'a str {
        type Listener = AddrIncoming;

        fn bind(self) -> crate::Result<Self::Listener> {
            self.parse::<SocketAddr>()?.bind()
        }
    }

    impl Bind for String {
        type Listener = AddrIncoming;

        fn bind(self) -> crate::Result<Self::Listener> {
            self.as_str().bind()
        }
    }
}

#[cfg(unix)]
pub mod unix {
    use {
        super::{sleep_on_errors::SleepOnErrors, *},
        crate::util::MapAsyncExt,
        std::{
            os::unix::net::SocketAddr,
            path::{Path, PathBuf},
            time::Duration,
        },
        tokio::net::{UnixListener, UnixStream},
    };

    impl Listener for UnixListener {
        type Conn = UnixStream;

        #[inline]
        fn poll_incoming(&mut self) -> Poll<(Self::Conn, RemoteAddr), io::Error> {
            self.poll_accept()
                .map_async(|(conn, addr)| (conn, addr.into()))
        }
    }

    /// An Unix domain socket transport that allows to sleep on accept errors.
    #[derive(Debug)]
    pub struct AddrIncoming {
        listener: SleepOnErrors<UnixListener>,
        local_addr: SocketAddr,
    }

    impl AddrIncoming {
        pub(crate) fn bind(sock_path: &Path) -> io::Result<Self> {
            let listener = UnixListener::bind(sock_path)?;
            let local_addr = listener.local_addr()?;
            Ok(Self {
                listener: SleepOnErrors::new(listener),
                local_addr,
            })
        }

        #[inline]
        pub fn local_addr(&self) -> &SocketAddr {
            &self.local_addr
        }

        #[inline]
        pub fn set_sleep_on_errors(&mut self, duration: Option<Duration>) -> &mut Self {
            self.listener.set_sleep_on_errors(duration);
            self
        }
    }

    impl Listener for AddrIncoming {
        type Conn = UnixStream;

        #[inline]
        fn poll_incoming(&mut self) -> Poll<(Self::Conn, RemoteAddr), io::Error> {
            self.listener.poll_incoming()
        }
    }

    impl Bind for PathBuf {
        type Listener = AddrIncoming;

        fn bind(self) -> crate::Result<Self::Listener> {
            self.as_path().bind()
        }
    }

    impl<'a> Bind for &'a PathBuf {
        type Listener = AddrIncoming;

        fn bind(self) -> crate::Result<Self::Listener> {
            self.as_path().bind()
        }
    }

    impl<'a> Bind for &'a Path {
        type Listener = AddrIncoming;

        fn bind(self) -> crate::Result<Self::Listener> {
            AddrIncoming::bind(&self).map_err(Into::into)
        }
    }
}

mod sleep_on_errors {
    use {
        super::*,
        futures::{Async, Future},
        std::time::{Duration, Instant},
        tokio::timer::Delay,
    };

    #[derive(Debug)]
    pub(crate) struct SleepOnErrors<T> {
        listener: T,
        duration: Option<Duration>,
        timeout: Option<Delay>,
    }

    impl<T> SleepOnErrors<T>
    where
        T: Listener,
    {
        pub(crate) fn new(listener: T) -> Self {
            Self {
                listener,
                duration: Some(Duration::from_secs(1)),
                timeout: None,
            }
        }

        pub(crate) fn set_sleep_on_errors(&mut self, duration: Option<Duration>) {
            self.duration = duration;
        }
    }

    impl<T> Listener for SleepOnErrors<T>
    where
        T: Listener,
    {
        type Conn = T::Conn;

        #[inline]
        fn poll_incoming(&mut self) -> Poll<(Self::Conn, RemoteAddr), io::Error> {
            if let Some(timeout) = &mut self.timeout {
                match timeout.poll() {
                    Ok(Async::Ready(())) => {}
                    Ok(Async::NotReady) => return Ok(Async::NotReady),
                    Err(timer_err) => log::error!("sleep timer error: {}", timer_err),
                }
                self.timeout = None;
            }

            loop {
                match self.listener.poll_incoming() {
                    Ok(ok) => return Ok(ok),
                    Err(ref err) if is_connection_error(err) => {
                        log::trace!("connection error: {}", err);
                        continue;
                    }
                    Err(err) => {
                        log::error!("accept error: {}", err);
                        if let Some(duration) = self.duration {
                            let mut timeout = Delay::new(Instant::now() + duration);
                            match timeout.poll() {
                                Ok(Async::Ready(())) => continue,
                                Ok(Async::NotReady) => {
                                    log::error!("sleep until {:?}", timeout.deadline());
                                    self.timeout = Some(timeout);
                                    return Ok(Async::NotReady);
                                }
                                Err(timer_err) => {
                                    log::error!("could not sleep: {}", timer_err);
                                }
                            }
                        }
                        return Err(err);
                    }
                }
            }
        }
    }

    /// Returns whether the kind of provided error is caused by connection to the peer.
    fn is_connection_error(err: &io::Error) -> bool {
        match err.kind() {
            io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset => true,
            _ => false,
        }
    }
}

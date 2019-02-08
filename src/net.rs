//! Abstraction around low-level network I/O.

use {
    futures::Poll,
    izanami_util::RemoteAddr,
    std::io,
    tokio::io::{AsyncRead, AsyncWrite},
};

/// A trait representing the conversion to an I/O object bound to the specified address.
pub trait Bind {
    /// The type of established connection.
    type Conn: AsyncRead + AsyncWrite;

    /// The type of listener returned from `bind`.
    type Listener: Listener<Conn = Self::Conn>;

    /// Create an I/O object bound to the specified address.
    fn bind(&self) -> crate::Result<Self::Listener>;
}

impl<F, T> Bind for F
where
    F: Fn() -> crate::Result<T>,
    T: Listener,
{
    type Conn = T::Conn;
    type Listener = T;

    #[inline]
    fn bind(&self) -> crate::Result<Self::Listener> {
        (*self)()
    }
}

/// A trait abstracting I/O objects that listens for incoming connections.
pub trait Listener {
    /// The type of established connection returned from `poll_incoming`.
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
        fn new(listener: TcpListener) -> io::Result<Self> {
            let local_addr = listener.local_addr()?;
            Ok(Self {
                listener: SleepOnErrors::new(listener),
                local_addr,
                tcp_keepalive_timeout: None,
                tcp_nodelay: false,
            })
        }

        pub(crate) fn bind(addr: &SocketAddr) -> io::Result<Self> {
            Self::new(TcpListener::bind(addr)?)
        }

        pub(crate) fn from_std(listener: std::net::TcpListener) -> io::Result<Self> {
            Self::new(TcpListener::from_std(
                listener,
                &tokio::reactor::Handle::default(),
            )?)
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
        type Conn = TcpStream;
        type Listener = AddrIncoming;

        fn bind(&self) -> crate::Result<Self::Listener> {
            AddrIncoming::bind(self).map_err(Into::into)
        }
    }

    impl<'a> Bind for &'a SocketAddr {
        type Conn = TcpStream;
        type Listener = AddrIncoming;

        fn bind(&self) -> crate::Result<Self::Listener> {
            AddrIncoming::bind(self).map_err(Into::into)
        }
    }

    impl<'a> Bind for &'a str {
        type Conn = TcpStream;
        type Listener = AddrIncoming;

        fn bind(&self) -> crate::Result<Self::Listener> {
            self.parse::<SocketAddr>()?.bind()
        }
    }

    impl Bind for String {
        type Conn = TcpStream;
        type Listener = AddrIncoming;

        fn bind(&self) -> crate::Result<Self::Listener> {
            self.parse::<SocketAddr>()?.bind()
        }
    }

    impl Bind for std::net::TcpListener {
        type Conn = TcpStream;
        type Listener = AddrIncoming;

        fn bind(&self) -> crate::Result<Self::Listener> {
            let listener = self.try_clone()?;
            Ok(AddrIncoming::from_std(listener)?)
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
        fn new(listener: UnixListener) -> io::Result<Self> {
            let local_addr = listener.local_addr()?;
            Ok(Self {
                listener: SleepOnErrors::new(listener),
                local_addr,
            })
        }

        pub(crate) fn bind(sock_path: &Path) -> io::Result<Self> {
            Self::new(UnixListener::bind(sock_path)?)
        }

        pub(crate) fn from_std(listener: std::os::unix::net::UnixListener) -> io::Result<Self> {
            Self::new(UnixListener::from_std(
                listener,
                &tokio::reactor::Handle::default(),
            )?)
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
        type Conn = UnixStream;
        type Listener = AddrIncoming;

        fn bind(&self) -> crate::Result<Self::Listener> {
            AddrIncoming::bind(&self).map_err(Into::into)
        }
    }

    impl<'a> Bind for &'a PathBuf {
        type Conn = UnixStream;
        type Listener = AddrIncoming;

        fn bind(&self) -> crate::Result<Self::Listener> {
            AddrIncoming::bind(&self).map_err(Into::into)
        }
    }

    impl<'a> Bind for &'a Path {
        type Conn = UnixStream;
        type Listener = AddrIncoming;

        fn bind(&self) -> crate::Result<Self::Listener> {
            AddrIncoming::bind(&self).map_err(Into::into)
        }
    }

    impl Bind for std::os::unix::net::UnixListener {
        type Conn = UnixStream;
        type Listener = AddrIncoming;

        fn bind(&self) -> crate::Result<Self::Listener> {
            let listener = self.try_clone()?;
            Ok(AddrIncoming::from_std(listener)?)
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

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
    fn into_listeners(self) -> io::Result<Vec<Self::Listener>>;
}

impl<T> Bind for T
where
    T: Listener,
{
    type Conn = T::Conn;
    type Listener = T;

    fn into_listeners(self) -> io::Result<Vec<Self::Listener>> {
        Ok(vec![self])
    }
}

impl<B> Bind for Vec<B>
where
    B: Bind,
{
    type Conn = B::Conn;
    type Listener = B::Listener;

    fn into_listeners(self) -> io::Result<Vec<Self::Listener>> {
        let mut listeners = vec![];
        for bind in self {
            listeners.extend(bind.into_listeners()?);
        }
        Ok(listeners)
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
        std::{
            net::{SocketAddr, ToSocketAddrs},
            time::Duration,
        },
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

        fn into_listeners(self) -> io::Result<Vec<Self::Listener>> {
            (&self).into_listeners()
        }
    }

    impl<'a> Bind for &'a SocketAddr {
        type Conn = TcpStream;
        type Listener = AddrIncoming;

        fn into_listeners(self) -> io::Result<Vec<Self::Listener>> {
            AddrIncoming::bind(self).map(|listener| vec![listener])
        }
    }

    impl<'a> Bind for &'a str {
        type Conn = TcpStream;
        type Listener = AddrIncoming;

        fn into_listeners(self) -> io::Result<Vec<Self::Listener>> {
            self.to_socket_addrs()?
                .map(|addr| AddrIncoming::bind(&addr))
                .collect()
        }
    }

    impl Bind for String {
        type Conn = TcpStream;
        type Listener = AddrIncoming;

        fn into_listeners(self) -> io::Result<Vec<Self::Listener>> {
            self.as_str().into_listeners()
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

        fn into_listeners(self) -> io::Result<Vec<Self::Listener>> {
            self.as_path().into_listeners()
        }
    }

    impl<'a> Bind for &'a PathBuf {
        type Conn = UnixStream;
        type Listener = AddrIncoming;

        fn into_listeners(self) -> io::Result<Vec<Self::Listener>> {
            self.as_path().into_listeners()
        }
    }

    impl<'a> Bind for &'a Path {
        type Conn = UnixStream;
        type Listener = AddrIncoming;

        fn into_listeners(self) -> io::Result<Vec<Self::Listener>> {
            AddrIncoming::bind(self).map(|listener| vec![listener])
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

    #[cfg(test)]
    mod tests {
        use {super::*, tokio::util::FutureExt};

        type DummyConnection = io::Cursor<Vec<u8>>;

        struct DummyListener {
            inner: std::collections::VecDeque<io::Result<DummyConnection>>,
        }

        impl Listener for DummyListener {
            type Conn = DummyConnection;

            fn poll_incoming(&mut self) -> Poll<(Self::Conn, RemoteAddr), io::Error> {
                let conn = self.inner.pop_front().expect("queue is empty")?;
                let addr = RemoteAddr::unknown();
                Ok((conn, addr).into())
            }
        }

        #[test]
        fn ignore_connection_errors() -> io::Result<()> {
            let listener = DummyListener {
                inner: vec![
                    Err(io::ErrorKind::ConnectionAborted.into()),
                    Err(io::ErrorKind::ConnectionRefused.into()),
                    Err(io::ErrorKind::ConnectionReset.into()),
                    Ok(io::Cursor::new(vec![])),
                ]
                .into_iter()
                .collect(),
            };

            let mut listener = SleepOnErrors::new(listener);
            listener.set_sleep_on_errors(Some(Duration::from_micros(1)));

            let mut rt = tokio::runtime::current_thread::Runtime::new()?;

            let result = rt.block_on(
                futures::future::poll_fn({
                    let listener = &mut listener;
                    move || listener.poll_incoming()
                })
                .timeout(Duration::from_millis(1)),
            );
            assert!(result.is_ok());

            Ok(())
        }

        #[test]
        fn sleep_on_errors() -> io::Result<()> {
            let listener = DummyListener {
                inner: vec![
                    Err(io::Error::new(io::ErrorKind::Other, "Too many open files")),
                    Ok(io::Cursor::new(vec![])),
                ]
                .into_iter()
                .collect(),
            };

            let mut listener = SleepOnErrors::new(listener);
            listener.set_sleep_on_errors(Some(Duration::from_micros(1)));

            let mut rt = tokio::runtime::current_thread::Runtime::new()?;

            let result = rt.block_on(
                futures::future::poll_fn({
                    let listener = &mut listener;
                    move || listener.poll_incoming()
                })
                .timeout(Duration::from_millis(1)),
            );
            assert!(result.is_ok());

            Ok(())
        }

        #[test]
        fn abort_on_errors() -> io::Result<()> {
            let listener = DummyListener {
                inner: vec![
                    Err(io::Error::new(io::ErrorKind::Other, "Too many open files")),
                    Ok(io::Cursor::new(vec![])),
                ]
                .into_iter()
                .collect(),
            };

            let mut listener = SleepOnErrors::new(listener);
            listener.set_sleep_on_errors(None);

            let mut rt = tokio::runtime::current_thread::Runtime::new()?;

            let result = rt.block_on(
                futures::future::poll_fn({
                    let listener = &mut listener;
                    move || listener.poll_incoming()
                })
                .timeout(Duration::from_millis(1)),
            );
            assert!(result.err().expect("should be failed").is_inner());

            Ok(())
        }
    }
}

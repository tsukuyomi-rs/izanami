//! Abstraction around low-level network I/O.

pub mod tcp {
    use {
        super::sleep_on_errors::{Listener, SleepOnErrors},
        crate::util::*,
        futures::{Poll, Stream},
        std::{
            io,
            net::{SocketAddr, TcpListener as StdTcpListener, ToSocketAddrs},
            time::Duration,
        },
        tokio::{
            io::{AsyncRead, AsyncWrite},
            net::{TcpListener, TcpStream},
            reactor::Handle,
        },
    };

    impl Listener for TcpListener {
        type Conn = (TcpStream, SocketAddr);
        #[inline]
        fn poll_accept(&mut self) -> Poll<Self::Conn, io::Error> {
            self.poll_accept()
        }
    }

    /// A TCP stream connected to a remote endpoint.
    #[derive(Debug)]
    pub struct AddrStream {
        stream: TcpStream,
        remote_addr: SocketAddr,
    }

    impl AddrStream {
        /// Returns the remote peer's address returned from `TcpListener::poll_accept`.
        pub fn remote_addr(&self) -> SocketAddr {
            self.remote_addr
        }
    }

    impl io::Read for AddrStream {
        #[inline]
        fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
            self.stream.read(dst)
        }
    }

    impl io::Write for AddrStream {
        #[inline]
        fn write(&mut self, src: &[u8]) -> io::Result<usize> {
            self.stream.write(src)
        }

        #[inline]
        fn flush(&mut self) -> io::Result<()> {
            self.stream.flush()
        }
    }

    impl AsyncRead for AddrStream {}

    impl AsyncWrite for AddrStream {
        #[inline]
        fn shutdown(&mut self) -> Poll<(), io::Error> {
            AsyncWrite::shutdown(&mut self.stream)
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
        pub fn new(listener: TcpListener) -> io::Result<Self> {
            let local_addr = listener.local_addr()?;
            Ok(Self {
                listener: SleepOnErrors::new(listener),
                local_addr,
                tcp_keepalive_timeout: None,
                tcp_nodelay: false,
            })
        }

        pub fn bind<A>(addr: A) -> io::Result<Self>
        where
            A: ToSocketAddrs,
        {
            let std_listener = StdTcpListener::bind(addr)?;
            let listener = TcpListener::from_std(std_listener, &Handle::default())?;
            Self::new(listener)
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

    impl Stream for AddrIncoming {
        type Item = AddrStream;
        type Error = io::Error;

        fn poll(&mut self) -> Poll<Option<Self::Item>, io::Error> {
            self.listener
                .poll_accept() //
                .map_async(|(stream, remote_addr)| {
                    if let Some(timeout) = self.tcp_keepalive_timeout {
                        if let Err(e) = stream.set_keepalive(Some(timeout)) {
                            log::trace!("error trying to set TCP keepalive: {}", e);
                        }
                    }

                    if let Err(e) = stream.set_nodelay(self.tcp_nodelay) {
                        log::trace!("error trying to set TCP nodelay: {}", e);
                    }

                    Some(AddrStream {
                        stream,
                        remote_addr,
                    })
                })
        }
    }
}

#[cfg(unix)]
pub mod unix {
    use {
        super::sleep_on_errors::{Listener, SleepOnErrors},
        crate::util::MapAsyncExt,
        futures::{Poll, Stream},
        std::{io, os::unix::net::SocketAddr, path::Path, time::Duration},
        tokio::{
            io::{AsyncRead, AsyncWrite},
            net::{UnixListener, UnixStream},
        },
    };

    impl Listener for UnixListener {
        type Conn = (UnixStream, SocketAddr);
        fn poll_accept(&mut self) -> Poll<Self::Conn, io::Error> {
            UnixListener::poll_accept(self)
        }
    }

    /// An Unix domain socket stream connected to a remote endpoint.
    #[derive(Debug)]
    pub struct AddrStream {
        stream: UnixStream,
        remote_addr: SocketAddr,
    }

    impl AddrStream {
        /// Returns the remote peer's address returned from `UnixListener::poll_accept`.
        pub fn remote_addr(&self) -> &SocketAddr {
            &self.remote_addr
        }
    }

    impl io::Read for AddrStream {
        #[inline]
        fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
            self.stream.read(dst)
        }
    }

    impl io::Write for AddrStream {
        #[inline]
        fn write(&mut self, src: &[u8]) -> io::Result<usize> {
            self.stream.write(src)
        }

        #[inline]
        fn flush(&mut self) -> io::Result<()> {
            self.stream.flush()
        }
    }

    impl AsyncRead for AddrStream {}

    impl AsyncWrite for AddrStream {
        #[inline]
        fn shutdown(&mut self) -> Poll<(), io::Error> {
            AsyncWrite::shutdown(&mut self.stream)
        }
    }

    /// An Unix domain socket transport that allows to sleep on accept errors.
    #[derive(Debug)]
    pub struct AddrIncoming {
        listener: SleepOnErrors<UnixListener>,
        local_addr: SocketAddr,
    }

    impl AddrIncoming {
        pub fn new(listener: UnixListener) -> io::Result<Self> {
            let local_addr = listener.local_addr()?;
            Ok(Self {
                listener: SleepOnErrors::new(listener),
                local_addr,
            })
        }

        pub fn bind<P>(path: P) -> io::Result<Self>
        where
            P: AsRef<Path>,
        {
            Self::new(UnixListener::bind(path)?)
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

    impl Stream for AddrIncoming {
        type Item = AddrStream;
        type Error = io::Error;

        #[inline]
        fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            self.listener
                .poll_accept()
                .map_async(|(stream, remote_addr)| {
                    Some(AddrStream {
                        stream,
                        remote_addr,
                    })
                })
        }
    }
}

mod sleep_on_errors {
    use {
        futures::{Async, Future, Poll},
        std::{
            io,
            time::{Duration, Instant},
        },
        tokio::timer::Delay,
    };

    pub(super) trait Listener {
        type Conn;
        fn poll_accept(&mut self) -> Poll<Self::Conn, io::Error>;
    }

    #[derive(Debug)]
    pub(super) struct SleepOnErrors<T> {
        listener: T,
        duration: Option<Duration>,
        timeout: Option<Delay>,
    }

    impl<T> SleepOnErrors<T>
    where
        T: Listener,
    {
        pub(super) fn new(listener: T) -> Self {
            Self {
                listener,
                duration: Some(Duration::from_secs(1)),
                timeout: None,
            }
        }

        pub(super) fn set_sleep_on_errors(&mut self, duration: Option<Duration>) {
            self.duration = duration;
        }

        #[inline]
        pub(super) fn poll_accept(&mut self) -> Poll<T::Conn, io::Error> {
            if let Some(timeout) = &mut self.timeout {
                match timeout.poll() {
                    Ok(Async::Ready(())) => {}
                    Ok(Async::NotReady) => return Ok(Async::NotReady),
                    Err(timer_err) => log::error!("sleep timer error: {}", timer_err),
                }
                self.timeout = None;
            }

            loop {
                match self.listener.poll_accept() {
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

            fn poll_accept(&mut self) -> Poll<Self::Conn, io::Error> {
                let conn = self.inner.pop_front().expect("queue is empty")?;
                Ok(conn.into())
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
                    move || listener.poll_accept()
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
                    move || listener.poll_accept()
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
                    move || listener.poll_accept()
                })
                .timeout(Duration::from_millis(1)),
            );
            assert!(result.err().expect("should be failed").is_inner());

            Ok(())
        }
    }
}

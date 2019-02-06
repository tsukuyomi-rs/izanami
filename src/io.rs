//! Abstraction around low-level I/O.

use {
    crate::tls::Acceptor,
    futures::{Async, Poll, Stream},
    izanami_util::RemoteAddr,
    std::{io, time::Duration},
    tokio::{
        io::{AsyncRead, AsyncWrite},
        timer::Delay,
    },
};

pub trait MakeListener {
    type Listener: Listener;
    type Error;

    fn make_listener(self) -> Result<Self::Listener, Self::Error>;
}

impl<T: Listener> MakeListener for T {
    type Listener = Self;
    type Error = io::Error; // FIXME: replace with `!`

    #[inline]
    fn make_listener(self) -> Result<Self::Listener, Self::Error> {
        Ok(self)
    }
}

/// A trait abstracting I/O objects that listens for incoming connections.
pub trait Listener {
    /// The type of established connection returned from `Incoming`.
    type Conn: AsyncRead + AsyncWrite;

    /// The type of incoming `Stream` that returns established connections.
    type Incoming: Stream<Item = Self::Conn, Error = io::Error>;

    /// Creates an incoming `Stream` that iterates over the connections.
    fn incoming(self) -> Self::Incoming;

    /// Retrieve the value of remote address from the provided connection.
    #[allow(unused_variables)]
    fn remote_addr(conn: &Self::Conn) -> RemoteAddr {
        RemoteAddr::unknown()
    }

    fn sleep_on_errors(self) -> SleepOnErrors<Self>
    where
        Self: Sized,
    {
        SleepOnErrors::new(self)
    }

    fn accept_with<A>(self, acceptor: A) -> AcceptWith<Self, A>
    where
        Self: Sized,
        A: Acceptor<Self::Conn>,
    {
        AcceptWith::new(self, acceptor)
    }
}

impl Listener for hyper::server::conn::AddrIncoming {
    type Conn = hyper::server::conn::AddrStream;
    type Incoming = Self;

    #[inline]
    fn incoming(self) -> Self::Incoming {
        self
    }

    #[inline]
    fn remote_addr(conn: &Self::Conn) -> RemoteAddr {
        conn.remote_addr().into()
    }
}

/// A wrapper for `Listener` that sleeps for the specified time interval
/// when occuring an accepting error.
#[derive(Debug)]
pub struct SleepOnErrors<T> {
    raw: T,
    duration: Duration,
    ignore_connection_errors: bool,
}

impl<T> SleepOnErrors<T>
where
    T: Listener,
{
    fn new(raw: T) -> Self {
        Self {
            raw,
            duration: Duration::from_secs(1),
            ignore_connection_errors: true,
        }
    }

    /// Returns a reference to the underlying listener.
    pub fn get_ref(&self) -> &T {
        &self.raw
    }

    /// Returns a mutable reference to the underlying listener.
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.raw
    }

    /// Sets the time interval when an accepting error occurs.
    ///
    /// The default value is `1sec`.
    pub fn duration(self, duration: Duration) -> Self {
        Self { duration, ..self }
    }

    /// Sets whether to ignore the connection errors or not.
    ///
    /// The default value is `true`.
    pub fn ignore_connection_errors(self, value: bool) -> Self {
        Self {
            ignore_connection_errors: value,
            ..self
        }
    }
}

impl<T> std::ops::Deref for SleepOnErrors<T>
where
    T: Listener,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.get_ref()
    }
}

impl<T> std::ops::DerefMut for SleepOnErrors<T>
where
    T: Listener,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.get_mut()
    }
}

mod sleep_on_errors {
    use {super::*, futures::Future, std::time::Instant};

    impl<T> Listener for SleepOnErrors<T>
    where
        T: Listener,
    {
        type Conn = T::Conn;
        type Incoming = Incoming<T::Incoming>;

        fn incoming(self) -> Self::Incoming {
            Incoming {
                incoming: self.raw.incoming(),
                duration: self.duration,
                ignore_connection_errors: self.ignore_connection_errors,
                timeout: None,
            }
        }

        fn remote_addr(conn: &Self::Conn) -> RemoteAddr {
            T::remote_addr(conn)
        }
    }

    #[derive(Debug)]
    pub struct Incoming<I> {
        incoming: I,
        duration: Duration,
        ignore_connection_errors: bool,
        timeout: Option<Delay>,
    }

    impl<I> Stream for Incoming<I>
    where
        I: Stream<Error = io::Error>,
    {
        type Item = I::Item;
        type Error = io::Error;

        #[inline]
        fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            if let Some(timeout) = &mut self.timeout {
                match timeout.poll() {
                    Ok(Async::Ready(())) => {}
                    Ok(Async::NotReady) => return Ok(Async::NotReady),
                    Err(timer_err) => log::error!("sleep timer error: {}", timer_err),
                }
                self.timeout = None;
            }

            loop {
                match self.incoming.poll() {
                    Ok(ok) => return Ok(ok),
                    Err(ref err) if self.ignore_connection_errors && is_connection_error(err) => {
                        log::debug!("connection error: {}", err);
                        continue;
                    }
                    Err(err) => {
                        log::error!("accept error: {}", err);
                        let mut timeout = Delay::new(Instant::now() + self.duration);
                        match timeout.poll() {
                            Ok(Async::Ready(())) => continue,
                            Ok(Async::NotReady) => {
                                log::error!("sleep until {:?}", timeout.deadline());
                                self.timeout = Some(timeout);
                                return Ok(Async::NotReady);
                            }
                            Err(timer_err) => {
                                log::error!("could not sleep: {}", timer_err);
                                return Err(err);
                            }
                        }
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

/// A wrapper for `Listener` that modifies the I/O returned from the incoming stream
/// using the specified `Acceptor`.
#[derive(Debug)]
pub struct AcceptWith<T, A> {
    listener: T,
    acceptor: A,
}

impl<T, A> AcceptWith<T, A>
where
    T: Listener,
    A: Acceptor<T::Conn>,
{
    pub(crate) fn new(listener: T, acceptor: A) -> Self {
        Self { listener, acceptor }
    }
}

mod with_acceptor {
    use super::*;

    impl<T, A> Listener for AcceptWith<T, A>
    where
        T: Listener,
        A: Acceptor<T::Conn>,
    {
        type Conn = AddrStream<A::Accepted>;
        type Incoming = Incoming<T, A>;

        fn incoming(self) -> Self::Incoming {
            Incoming {
                incoming: self.listener.incoming(),
                acceptor: self.acceptor,
            }
        }

        fn remote_addr(conn: &Self::Conn) -> RemoteAddr {
            conn.remote_addr().clone()
        }
    }

    #[allow(missing_debug_implementations)]
    pub struct Incoming<T: Listener, A> {
        incoming: T::Incoming,
        acceptor: A,
    }

    impl<T, A> Stream for Incoming<T, A>
    where
        T: Listener,
        A: Acceptor<T::Conn>,
    {
        type Item = AddrStream<A::Accepted>;
        type Error = io::Error;

        fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            let io_opt = futures::try_ready!(self.incoming.poll());
            Ok(Async::Ready(io_opt.map(|io| {
                let remote_addr = T::remote_addr(&io);
                AddrStream::new(self.acceptor.accept(io), remote_addr)
            })))
        }
    }
}

/// Wrapper for asynchronouss I/O that holds the peer's address.
#[derive(Debug)]
pub struct AddrStream<T> {
    io: T,
    remote_addr: RemoteAddr,
}

impl<T> AddrStream<T>
where
    T: AsyncRead + AsyncWrite,
{
    fn new(io: T, remote_addr: RemoteAddr) -> Self {
        Self { io, remote_addr }
    }

    /// Returns a reference to the underlying I/O.
    pub fn get_ref(&self) -> &T {
        &self.io
    }

    /// Returns a mutable reference to the underlying I/O.
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.io
    }

    /// Returns a reference to the remote address acquired from the underlying I/O.
    pub fn remote_addr(&self) -> &RemoteAddr {
        &self.remote_addr
    }
}

impl<T> io::Read for AddrStream<T>
where
    T: AsyncRead + AsyncWrite,
{
    #[inline]
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        self.io.read(dst)
    }
}

impl<T> io::Write for AddrStream<T>
where
    T: AsyncRead + AsyncWrite,
{
    #[inline]
    fn write(&mut self, src: &[u8]) -> io::Result<usize> {
        self.io.write(src)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.io.flush()
    }
}

impl<T> AsyncRead for AddrStream<T>
where
    T: AsyncRead + AsyncWrite,
{
    #[inline]
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.io.prepare_uninitialized_buffer(buf)
    }
}

impl<T> AsyncWrite for AddrStream<T>
where
    T: AsyncRead + AsyncWrite,
{
    #[inline]
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.io.shutdown()
    }
}

mod tcp {
    use {
        super::*,
        tokio::net::{TcpListener, TcpStream},
    };

    impl MakeListener for std::net::SocketAddr {
        type Listener = SleepOnErrors<TcpListener>;
        type Error = io::Error;

        fn make_listener(self) -> Result<Self::Listener, Self::Error> {
            (&self).make_listener()
        }
    }

    impl<'a> MakeListener for &'a std::net::SocketAddr {
        type Listener = SleepOnErrors<TcpListener>;
        type Error = io::Error;

        fn make_listener(self) -> Result<Self::Listener, Self::Error> {
            TcpListener::bind(self).map(SleepOnErrors::new)
        }
    }

    impl<'a> MakeListener for &'a str {
        type Listener = SleepOnErrors<TcpListener>;
        type Error = io::Error;

        fn make_listener(self) -> Result<Self::Listener, Self::Error> {
            let addr: std::net::SocketAddr = self
                .parse()
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            addr.make_listener()
        }
    }

    impl MakeListener for String {
        type Listener = SleepOnErrors<TcpListener>;
        type Error = io::Error;

        fn make_listener(self) -> Result<Self::Listener, Self::Error> {
            self.as_str().make_listener()
        }
    }

    impl Listener for TcpListener {
        type Conn = AddrStream<TcpStream>;
        type Incoming = Incoming;

        #[inline]
        fn incoming(self) -> Self::Incoming {
            Incoming { listener: self }
        }

        #[inline]
        fn remote_addr(conn: &Self::Conn) -> RemoteAddr {
            conn.remote_addr.clone()
        }
    }

    #[derive(Debug)]
    pub struct Incoming {
        listener: TcpListener,
    }

    impl Stream for Incoming {
        type Item = AddrStream<TcpStream>;
        type Error = io::Error;

        #[inline]
        fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            let (io, addr) = futures::try_ready!(self.listener.poll_accept());
            Ok(Some(AddrStream {
                io,
                remote_addr: addr.into(),
            })
            .into())
        }
    }
}

#[cfg(unix)]
mod uds {
    use {
        super::*,
        tokio::net::{UnixListener, UnixStream},
    };

    impl MakeListener for std::path::PathBuf {
        type Listener = SleepOnErrors<UnixListener>;
        type Error = io::Error;

        fn make_listener(self) -> Result<Self::Listener, Self::Error> {
            self.as_path().make_listener()
        }
    }

    impl<'a> MakeListener for &'a std::path::Path {
        type Listener = SleepOnErrors<UnixListener>;
        type Error = io::Error;

        fn make_listener(self) -> Result<Self::Listener, Self::Error> {
            UnixListener::bind(&self).map(SleepOnErrors::new)
        }
    }

    impl Listener for UnixListener {
        type Conn = AddrStream<UnixStream>;
        type Incoming = Incoming;

        #[inline]
        fn incoming(self) -> Self::Incoming {
            Incoming { listener: self }
        }

        #[inline]
        fn remote_addr(conn: &Self::Conn) -> RemoteAddr {
            conn.remote_addr.clone()
        }
    }

    #[derive(Debug)]
    pub struct Incoming {
        listener: UnixListener,
    }

    impl Stream for Incoming {
        type Item = AddrStream<UnixStream>;
        type Error = io::Error;

        #[inline]
        fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            let (io, addr) = futures::try_ready!(self.listener.poll_accept());
            Ok(Some(AddrStream {
                io,
                remote_addr: addr.into(),
            })
            .into())
        }
    }
}

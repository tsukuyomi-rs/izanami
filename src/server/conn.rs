//! Abstraction around low-level I/O.

use {
    futures::{Async, Poll, Stream},
    izanami_util::RemoteAddr,
    std::{io, time::Duration},
    tokio::{
        io::{AsyncRead, AsyncWrite},
        timer::Delay,
    },
};

/// A trait that represents the listener.
pub trait Listener {
    /// The type of connection to the peer returned from `Incoming`.
    type Conn: AsyncRead + AsyncWrite;

    /// The type of incoming `Stream` that returns the connections with the peer.
    type Incoming: Stream<Item = Self::Conn, Error = io::Error>;

    /// Consume itself and creates an incoming `Stream` of asynchronous I/Os.
    fn incoming(self) -> Self::Incoming;

    /// Retrieve the value of remote address from the provided connection.
    #[allow(unused_variables)]
    fn remote_addr(conn: &Self::Conn) -> RemoteAddr {
        RemoteAddr::unknown()
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

/// A trait that represents the conversion of asynchronous I/Os.
///
/// Typically, the implementors of this trait establish a TLS session.
pub trait Acceptor<T> {
    type Accepted: AsyncRead + AsyncWrite;

    /// Converts the supplied I/O object into an `Accepted`.
    ///
    /// The returned I/O from this method includes the handshake process,
    /// and the process will be executed by reading/writing the I/O.
    fn accept(&self, io: T) -> Self::Accepted;
}

impl<F, T, U> Acceptor<T> for F
where
    F: Fn(T) -> U,
    U: AsyncRead + AsyncWrite,
{
    type Accepted = U;

    #[inline]
    fn accept(&self, io: T) -> Self::Accepted {
        (*self)(io)
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
    pub(crate) fn new(raw: T) -> Self {
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
    pub fn set_duration(&mut self, value: Duration) {
        self.duration = value;
    }

    /// Sets whether to ignore the connection errors or not.
    ///
    /// The default value is `true`.
    pub fn ignore_connection_errors(&mut self, value: bool) {
        self.ignore_connection_errors = value;
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

/// A wrapper for `Listener` that modifies the I/O returned from the incoming stream
/// using the specified `Acceptor`.
#[derive(Debug)]
pub struct WithAcceptor<T, A> {
    listener: T,
    acceptor: A,
}

impl<T, A> WithAcceptor<T, A>
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

    impl<T, A> Listener for WithAcceptor<T, A>
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
                AddrStream {
                    io: self.acceptor.accept(io),
                    remote_addr,
                }
            })))
        }
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

#[cfg(feature = "native-tls")]
mod use_navite_tls {
    use {
        super::*,
        futures::Poll,
        native_tls::{HandshakeError, TlsAcceptor, TlsStream},
        std::io,
    };

    impl<T> Acceptor<T> for TlsAcceptor
    where
        T: AsyncRead + AsyncWrite,
    {
        type Accepted = TlsStreamWithHandshake<T>;

        #[inline]
        fn accept(&self, io: T) -> Self::Accepted {
            TlsStreamWithHandshake::MidHandshake(MidHandshake(Some(self.accept(io))))
        }
    }

    #[derive(Debug)]
    pub struct MidHandshake<S>(Option<Result<TlsStream<S>, HandshakeError<S>>>);

    impl<S> MidHandshake<S>
    where
        S: AsyncRead + AsyncWrite,
    {
        fn try_handshake(&mut self) -> io::Result<TlsStream<S>> {
            match self.0.take().expect("unexpected condition") {
                Ok(io) => Ok(io),
                Err(HandshakeError::Failure(err)) => Err(io::Error::new(io::ErrorKind::Other, err)),
                Err(HandshakeError::WouldBlock(m)) => match m.handshake() {
                    Ok(io) => Ok(io),
                    Err(HandshakeError::Failure(err)) => {
                        Err(io::Error::new(io::ErrorKind::Other, err))
                    }
                    Err(HandshakeError::WouldBlock(s)) => {
                        self.0 = Some(Err(HandshakeError::WouldBlock(s)));
                        Err(io::ErrorKind::WouldBlock.into())
                    }
                },
            }
        }
    }

    #[derive(Debug)]
    pub enum TlsStreamWithHandshake<S> {
        MidHandshake(MidHandshake<S>),
        Ready(TlsStream<S>),
        Gone,
    }

    impl<S> TlsStreamWithHandshake<S>
    where
        S: AsyncRead + AsyncWrite,
    {
        fn ready(io: TlsStream<S>) -> Self {
            TlsStreamWithHandshake::Ready(io)
        }
    }

    impl<S> io::Read for TlsStreamWithHandshake<S>
    where
        S: AsyncRead + AsyncWrite,
    {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            loop {
                *self = match self {
                    TlsStreamWithHandshake::MidHandshake(m) => {
                        m.try_handshake().map(Self::ready)?
                    }
                    TlsStreamWithHandshake::Ready(io) => return io.read(buf),
                    TlsStreamWithHandshake::Gone => return Err(io::ErrorKind::Other.into()),
                };
            }
        }
    }

    impl<S> io::Write for TlsStreamWithHandshake<S>
    where
        S: AsyncRead + AsyncWrite,
    {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            loop {
                *self = match self {
                    TlsStreamWithHandshake::MidHandshake(m) => {
                        m.try_handshake().map(Self::ready)?
                    }
                    TlsStreamWithHandshake::Ready(io) => return io.write(buf),
                    TlsStreamWithHandshake::Gone => return Err(io::ErrorKind::Other.into()),
                };
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            loop {
                *self = match self {
                    TlsStreamWithHandshake::MidHandshake(m) => {
                        if m.0.is_none() {
                            return Ok(());
                        }
                        m.try_handshake().map(Self::ready)?
                    }
                    TlsStreamWithHandshake::Ready(io) => return io.flush(),
                    TlsStreamWithHandshake::Gone => return Err(io::ErrorKind::Other.into()),
                }
            }
        }
    }

    impl<S> AsyncRead for TlsStreamWithHandshake<S> where S: AsyncRead + AsyncWrite {}

    impl<S> AsyncWrite for TlsStreamWithHandshake<S>
    where
        S: AsyncRead + AsyncWrite,
    {
        fn shutdown(&mut self) -> Poll<(), io::Error> {
            match self {
                TlsStreamWithHandshake::MidHandshake(..) => {
                    *self = TlsStreamWithHandshake::Gone;
                    Ok(().into())
                }
                TlsStreamWithHandshake::Ready(io) => {
                    tokio_io::try_nb!(io.shutdown());
                    io.get_mut().shutdown()
                }
                TlsStreamWithHandshake::Gone => Ok(().into()),
            }
        }
    }
}

#[cfg(feature = "rustls")]
mod use_rustls {
    use {
        super::*,
        futures::Poll,
        rustls::{ServerConfig, ServerSession, Session, Stream},
        std::{io, sync::Arc},
    };

    impl<T> Acceptor<T> for Arc<ServerConfig>
    where
        T: AsyncRead + AsyncWrite,
    {
        type Accepted = TlsStream<T>;

        #[inline]
        fn accept(&self, io: T) -> Self::Accepted {
            TlsStream {
                io,
                is_shutdown: false,
                session: ServerSession::new(self),
            }
        }
    }

    #[derive(Debug)]
    pub struct TlsStream<S> {
        io: S,
        is_shutdown: bool,
        session: ServerSession,
    }

    impl<S> TlsStream<S>
    where
        S: AsyncRead + AsyncWrite,
    {
        #[inline]
        fn stream(&mut self) -> Stream<'_, ServerSession, S> {
            Stream::new(&mut self.session, &mut self.io)
        }
    }

    impl<S> io::Read for TlsStream<S>
    where
        S: AsyncRead + AsyncWrite,
    {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.stream().read(buf)
        }
    }

    impl<S> io::Write for TlsStream<S>
    where
        S: AsyncRead + AsyncWrite,
    {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.stream().write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.stream().flush()?;
            self.io.flush()
        }
    }

    impl<S> AsyncRead for TlsStream<S> where S: AsyncRead + AsyncWrite {}

    impl<S> AsyncWrite for TlsStream<S>
    where
        S: AsyncRead + AsyncWrite,
    {
        fn shutdown(&mut self) -> Poll<(), io::Error> {
            if self.session.is_handshaking() {
                return Ok(().into());
            }

            if !self.is_shutdown {
                self.session.send_close_notify();
                self.is_shutdown = true;
            }

            tokio_io::try_nb!(io::Write::flush(self));
            self.io.shutdown()
        }
    }
}

#[cfg(feature = "openssl")]
mod use_openssl {
    use {
        super::*,
        futures::{Async, Poll},
        openssl::ssl::{
            ErrorCode, //
            HandshakeError,
            MidHandshakeSslStream,
            ShutdownResult,
            SslAcceptor,
            SslStream,
        },
        std::io,
    };

    impl<T> Acceptor<T> for SslAcceptor
    where
        T: AsyncRead + AsyncWrite,
    {
        type Accepted = SslStreamWithHandshake<T>;

        #[inline]
        fn accept(&self, io: T) -> Self::Accepted {
            SslStreamWithHandshake::start(self.clone(), io)
        }
    }

    #[allow(missing_debug_implementations)]
    enum MidHandshake<T> {
        Start { acceptor: SslAcceptor, io: T },
        Handshake(MidHandshakeSslStream<T>),
        Done,
    }

    impl<T> MidHandshake<T>
    where
        T: AsyncRead + AsyncWrite,
    {
        fn try_handshake(&mut self) -> io::Result<Option<SslStream<T>>> {
            match std::mem::replace(self, MidHandshake::Done) {
                MidHandshake::Start { acceptor, io } => match acceptor.accept(io) {
                    Ok(io) => Ok(Some(io)),
                    Err(HandshakeError::WouldBlock(s)) => {
                        *self = MidHandshake::Handshake(s);
                        Err(io::ErrorKind::WouldBlock.into())
                    }
                    Err(_e) => Err(io::Error::new(io::ErrorKind::Other, "handshake error")),
                },
                MidHandshake::Handshake(s) => match s.handshake() {
                    Ok(io) => Ok(Some(io)),
                    Err(HandshakeError::WouldBlock(s)) => {
                        *self = MidHandshake::Handshake(s);
                        Err(io::ErrorKind::WouldBlock.into())
                    }
                    Err(_e) => Err(io::Error::new(io::ErrorKind::Other, "handshake error")),
                },
                MidHandshake::Done => Ok(None),
            }
        }
    }

    #[allow(missing_debug_implementations)]
    pub struct SslStreamWithHandshake<S> {
        state: State<S>,
    }

    #[allow(missing_debug_implementations)]
    enum State<S> {
        MidHandshake(MidHandshake<S>),
        Ready(SslStream<S>),
        Gone,
    }

    impl<S> SslStreamWithHandshake<S>
    where
        S: AsyncRead + AsyncWrite,
    {
        fn start(acceptor: SslAcceptor, io: S) -> Self {
            SslStreamWithHandshake {
                state: State::MidHandshake(MidHandshake::Start { acceptor, io }),
            }
        }
    }

    impl<S> io::Read for SslStreamWithHandshake<S>
    where
        S: AsyncRead + AsyncWrite,
    {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            loop {
                self.state = match &mut self.state {
                    State::MidHandshake(m) => match m.try_handshake()? {
                        Some(io) => State::Ready(io),
                        None => panic!("cannot perform handshake twice"),
                    },
                    State::Ready(io) => return io.read(buf),
                    State::Gone => return Err(io::ErrorKind::Other.into()),
                };
            }
        }
    }

    impl<S> io::Write for SslStreamWithHandshake<S>
    where
        S: AsyncRead + AsyncWrite,
    {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            loop {
                self.state = match &mut self.state {
                    State::MidHandshake(m) => match m.try_handshake()? {
                        Some(io) => State::Ready(io),
                        None => panic!("cannot perform handshake twice"),
                    },
                    State::Ready(io) => return io.write(buf),
                    State::Gone => return Err(io::ErrorKind::Other.into()),
                };
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            loop {
                self.state = match &mut self.state {
                    State::MidHandshake(m) => match m.try_handshake()? {
                        Some(io) => State::Ready(io),
                        None => return Ok(()),
                    },
                    State::Ready(io) => return io.flush(),
                    State::Gone => return Err(io::ErrorKind::Other.into()),
                };
            }
        }
    }

    impl<S> AsyncRead for SslStreamWithHandshake<S>
    where
        S: AsyncRead + AsyncWrite,
    {
        unsafe fn prepare_uninitialized_buffer(&self, _: &mut [u8]) -> bool {
            true
        }
    }

    impl<S> AsyncWrite for SslStreamWithHandshake<S>
    where
        S: AsyncRead + AsyncWrite,
    {
        fn shutdown(&mut self) -> Poll<(), io::Error> {
            match &mut self.state {
                State::MidHandshake(..) => {
                    self.state = State::Gone;
                    Ok(Async::Ready(()))
                }
                State::Ready(io) => match io.shutdown() {
                    Ok(ShutdownResult::Sent) | Ok(ShutdownResult::Received) => Ok(Async::Ready(())),
                    Err(ref e) if e.code() == ErrorCode::ZERO_RETURN => Ok(Async::Ready(())),
                    Err(ref e)
                        if e.code() == ErrorCode::WANT_READ
                            || e.code() == ErrorCode::WANT_WRITE =>
                    {
                        Ok(Async::NotReady)
                    }
                    Err(e) => Err(e
                        .into_io_error()
                        .unwrap_or_else(|e| io::Error::new(io::ErrorKind::Other, e))),
                },
                State::Gone => Ok(().into()),
            }
        }
    }
}

//! Abstraction around low-level I/O.

use {
    futures::{Async, Poll, Stream},
    std::{io, net::SocketAddr, time::Duration},
    tokio::{
        io::{AsyncRead, AsyncWrite},
        timer::Delay,
    },
};

/// A trait that represents the listener.
pub trait Transport {
    /// The type of connection to the peer returned from `Incoming`.
    type Conn: AsyncRead + AsyncWrite;

    /// The type of incoming `Stream` that returns the connections with the peer.
    type Incoming: Stream<Item = Self::Conn, Error = io::Error>;

    /// Consume itself and creates an incoming `Stream` of asynchronous I/Os.
    fn incoming(self) -> Self::Incoming;

    #[doc(hidden)]
    #[allow(unused_variables)]
    fn remote_addr(conn: &Self::Conn) -> Option<SocketAddr> {
        None
    }
}

impl Transport for hyper::server::conn::AddrIncoming {
    type Conn = hyper::server::conn::AddrStream;
    type Incoming = Self;

    #[inline]
    fn incoming(self) -> Self::Incoming {
        self
    }

    #[inline]
    fn remote_addr(conn: &Self::Conn) -> Option<SocketAddr> {
        Some(conn.remote_addr())
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

impl<T> Acceptor<T> for ()
where
    T: AsyncRead + AsyncWrite,
{
    type Accepted = T;

    #[inline]
    fn accept(&self, io: T) -> Self::Accepted {
        io
    }
}

/// An instance of `Transport` used in `Server` by default.
#[derive(Debug)]
pub struct DefaultTransport<T, A = ()> {
    transport: T,
    acceptor: A,
    sleep_on_errors: Option<Duration>,
}

impl<T, A> DefaultTransport<T, A>
where
    T: Transport,
    A: Acceptor<T::Conn>,
{
    pub(crate) fn new(transport: T, acceptor: A) -> Self {
        Self {
            transport,
            acceptor,
            sleep_on_errors: Some(Duration::from_secs(1)),
        }
    }

    pub fn get_ref(&self) -> (&T, &A) {
        (&self.transport, &self.acceptor)
    }

    pub fn get_mut(&mut self) -> (&mut T, &mut A) {
        (&mut self.transport, &mut self.acceptor)
    }

    pub fn accept<A2>(self, acceptor: A2) -> DefaultTransport<T, A2>
    where
        A2: Acceptor<T::Conn>,
    {
        DefaultTransport {
            transport: self.transport,
            acceptor,
            sleep_on_errors: self.sleep_on_errors,
        }
    }

    pub fn sleep_on_errors(self, duration: Option<Duration>) -> Self {
        Self {
            sleep_on_errors: duration,
            ..self
        }
    }
}

/// An asynchronous I/O that will hold the peer's address.
#[derive(Debug)]
pub struct AddrStream<T> {
    io: T,
    remote_addr: Option<SocketAddr>,
}

impl<T> AddrStream<T>
where
    T: AsyncRead + AsyncWrite,
{
    pub fn get_ref(&self) -> &T {
        &self.io
    }

    pub fn get_mut(&mut self) -> &mut T {
        &mut self.io
    }

    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.remote_addr
    }
}

impl<T> io::Read for AddrStream<T>
where
    T: AsyncRead + AsyncWrite,
{
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        self.io.read(dst)
    }
}

impl<T> io::Write for AddrStream<T>
where
    T: AsyncRead + AsyncWrite,
{
    fn write(&mut self, src: &[u8]) -> io::Result<usize> {
        self.io.write(src)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.io.flush()
    }
}

impl<T> AsyncRead for AddrStream<T>
where
    T: AsyncRead + AsyncWrite,
{
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.io.prepare_uninitialized_buffer(buf)
    }
}

impl<T> AsyncWrite for AddrStream<T>
where
    T: AsyncRead + AsyncWrite,
{
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.io.shutdown()
    }
}

mod default {
    use {super::*, futures::Future};

    impl<T, A> Transport for DefaultTransport<T, A>
    where
        T: Transport,
        A: Acceptor<T::Conn>,
    {
        type Conn = AddrStream<A::Accepted>;
        type Incoming = Incoming<T, A>;

        fn incoming(self) -> Self::Incoming {
            Incoming {
                incoming: self.transport.incoming(),
                acceptor: self.acceptor,
                sleep_on_errors: self.sleep_on_errors,
                timeout: None,
            }
        }

        fn remote_addr(conn: &Self::Conn) -> Option<SocketAddr> {
            conn.remote_addr
        }
    }

    #[derive(Debug)]
    pub struct Incoming<T: Transport, A> {
        incoming: T::Incoming,
        acceptor: A,
        sleep_on_errors: Option<Duration>,
        timeout: Option<Delay>,
    }

    impl<T, A> Stream for Incoming<T, A>
    where
        T: Transport,
        A: Acceptor<T::Conn>,
    {
        type Item = AddrStream<A::Accepted>;
        type Error = io::Error;

        #[inline]
        fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            if let Some(timeout) = &mut self.timeout {
                match timeout.poll() {
                    Ok(Async::Ready(())) => {}
                    Ok(Async::NotReady) => return Ok(Async::NotReady),
                    Err(err) => log::error!("sleep timer error: {}", err),
                }
            }
            self.timeout = None;

            loop {
                match self.incoming.poll() {
                    Ok(Async::Ready(io_opt)) => {
                        return Ok(Async::Ready(io_opt.map(|io| {
                            let remote_addr = T::remote_addr(&io);
                            AddrStream {
                                io: self.acceptor.accept(io),
                                remote_addr,
                            }
                        })));
                    }
                    Ok(Async::NotReady) => return Ok(Async::NotReady),
                    Err(err) => {
                        match err.kind() {
                            io::ErrorKind::ConnectionRefused
                            | io::ErrorKind::ConnectionAborted
                            | io::ErrorKind::ConnectionReset => {
                                log::debug!("connection error: {}", err);
                                continue;
                            }
                            _ => {}
                        }

                        if let Some(duration) = self.sleep_on_errors {
                            let delay = std::time::Instant::now() + duration;
                            let mut timeout = Delay::new(delay);
                            match timeout.poll() {
                                Ok(Async::Ready(())) => {
                                    log::error!("accept error: {}", err);
                                    continue;
                                }
                                Ok(Async::NotReady) => {
                                    log::error!("accept error: {}", err);
                                    self.timeout = Some(timeout);
                                    return Ok(Async::NotReady);
                                }
                                Err(timer_err) => {
                                    log::error!("could not sleep on error: {}", timer_err);
                                    return Err(err);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

mod tcp {
    use {
        super::*,
        tokio::net::{tcp::Incoming, TcpListener, TcpStream},
    };

    impl Transport for TcpListener {
        type Conn = TcpStream;
        type Incoming = Incoming;

        #[inline]
        fn incoming(self) -> Self::Incoming {
            self.incoming()
        }

        #[inline]
        fn remote_addr(conn: &Self::Conn) -> Option<SocketAddr> {
            conn.peer_addr().ok()
        }
    }
}

#[cfg(unix)]
mod uds {
    use {
        super::Transport,
        tokio::net::{unix::Incoming, UnixListener, UnixStream},
    };

    impl Transport for UnixListener {
        type Conn = UnixStream;
        type Incoming = Incoming;

        #[inline]
        fn incoming(self) -> Self::Incoming {
            self.incoming()
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

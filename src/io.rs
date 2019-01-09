use {
    futures::{Async, Future, Poll, Stream},
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
    fn listen(self) -> io::Result<Self::Incoming>;
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

#[derive(Debug)]
pub struct Incoming<S, A> {
    stream: S,
    acceptor: A,
    sleep_on_errors: Option<Duration>,
    timeout: Option<Delay>,
}

impl<S, A> Incoming<S, A>
where
    S: Stream<Error = io::Error>,
    A: Acceptor<S::Item>,
{
    pub(crate) fn new(stream: S, acceptor: A, sleep_on_errors: Option<Duration>) -> Self {
        Incoming {
            stream,
            acceptor,
            sleep_on_errors,
            timeout: None,
        }
    }
}

impl<S, A> Stream for Incoming<S, A>
where
    S: Stream<Error = io::Error>,
    A: Acceptor<S::Item>,
{
    type Item = A::Accepted;
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
            match self.stream.poll() {
                Ok(Async::Ready(io_opt)) => {
                    return Ok(Async::Ready(io_opt.map(|io| self.acceptor.accept(io))))
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

mod tcp {
    use {
        super::Listener,
        std::{io, net::SocketAddr},
        tokio::{
            net::{tcp::Incoming, TcpListener, TcpStream},
            reactor::Handle,
        },
    };

    impl Listener for SocketAddr {
        type Conn = TcpStream;
        type Incoming = Incoming;

        #[inline]
        fn listen(self) -> io::Result<Self::Incoming> {
            (&self).listen()
        }
    }

    impl<'a> Listener for &'a SocketAddr {
        type Conn = TcpStream;
        type Incoming = Incoming;

        #[inline]
        fn listen(self) -> io::Result<Self::Incoming> {
            Ok(TcpListener::bind(self)?.incoming())
        }
    }

    impl Listener for std::net::TcpListener {
        type Conn = TcpStream;
        type Incoming = Incoming;

        #[inline]
        fn listen(self) -> io::Result<Self::Incoming> {
            let listener = TcpListener::from_std(self, &Handle::default())?;
            Ok(listener.incoming())
        }
    }

    impl Listener for TcpListener {
        type Conn = TcpStream;
        type Incoming = Incoming;

        #[inline]
        fn listen(self) -> io::Result<Self::Incoming> {
            Ok(self.incoming())
        }
    }

    impl Listener for hyper::server::conn::AddrIncoming {
        type Conn = hyper::server::conn::AddrStream;
        type Incoming = Self;

        #[inline]
        fn listen(self) -> io::Result<Self::Incoming> {
            Ok(self)
        }
    }
}

#[cfg(unix)]
mod uds {
    use {
        super::Listener,
        std::{
            io,
            path::{Path, PathBuf},
        },
        tokio::{
            net::{unix::Incoming, UnixListener, UnixStream},
            reactor::Handle,
        },
    };

    impl Listener for PathBuf {
        type Conn = UnixStream;
        type Incoming = Incoming;

        #[inline]
        fn listen(self) -> io::Result<Self::Incoming> {
            (&self).listen()
        }
    }

    impl<'a> Listener for &'a PathBuf {
        type Conn = UnixStream;
        type Incoming = Incoming;

        #[inline]
        fn listen(self) -> io::Result<Self::Incoming> {
            <&'a std::path::Path>::listen(&*self)
        }
    }

    impl<'a> Listener for &'a Path {
        type Conn = UnixStream;
        type Incoming = Incoming;

        #[inline]
        fn listen(self) -> io::Result<Self::Incoming> {
            Ok(UnixListener::bind(self)?.incoming())
        }
    }

    impl Listener for UnixListener {
        type Conn = UnixStream;
        type Incoming = Incoming;

        #[inline]
        fn listen(self) -> io::Result<Self::Incoming> {
            Ok(self.incoming())
        }
    }

    impl Listener for std::os::unix::net::UnixListener {
        type Conn = UnixStream;
        type Incoming = Incoming;

        #[inline]
        fn listen(self) -> io::Result<Self::Incoming> {
            Ok(UnixListener::from_std(self, &Handle::default())?.incoming())
        }
    }
}

#[cfg(feature = "use-native-tls")]
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

#[cfg(feature = "use-rustls")]
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

#[cfg(feature = "use-openssl")]
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

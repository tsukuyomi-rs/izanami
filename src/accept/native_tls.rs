#![cfg(feature = "native-tls")]

use {
    super::*,
    ::native_tls::{HandshakeError, TlsAcceptor, TlsStream},
    futures::Poll,
    std::io,
};

impl<T> Acceptor<T> for TlsAcceptor
where
    T: AsyncRead + AsyncWrite,
{
    type Accepted = WithHandshakeStream<T>;

    #[inline]
    fn accept(&self, io: T) -> Self::Accepted {
        WithHandshakeStream::new(io, self)
    }
}

#[derive(Debug)]
struct MidHandshake<S> {
    inner: Option<Result<TlsStream<S>, HandshakeError<S>>>,
}

impl<S> MidHandshake<S>
where
    S: AsyncRead + AsyncWrite,
{
    fn try_handshake(&mut self) -> io::Result<TlsStream<S>> {
        match self.inner.take().expect("unexpected condition") {
            Ok(io) => Ok(io),
            Err(HandshakeError::Failure(err)) => Err(io::Error::new(io::ErrorKind::Other, err)),
            Err(HandshakeError::WouldBlock(m)) => match m.handshake() {
                Ok(io) => Ok(io),
                Err(HandshakeError::Failure(err)) => Err(io::Error::new(io::ErrorKind::Other, err)),
                Err(HandshakeError::WouldBlock(s)) => {
                    self.inner = Some(Err(HandshakeError::WouldBlock(s)));
                    Err(io::ErrorKind::WouldBlock.into())
                }
            },
        }
    }

    /// Flush the inner stream, without progress the handshake process.
    fn flush(&mut self) -> io::Result<()> {
        match &mut self.inner {
            Some(Ok(io)) => io.get_mut().flush(),
            Some(Err(HandshakeError::WouldBlock(s))) => s.get_mut().flush(),
            Some(Err(HandshakeError::Failure(..))) => unreachable!("this is a bug"),
            None => Ok(()),
        }
    }
}

#[derive(Debug)]
pub struct WithHandshakeStream<S> {
    state: State<S>,
}

#[derive(Debug)]
enum State<S> {
    MidHandshake(MidHandshake<S>),
    Ready(TlsStream<S>),
    Gone,
}

impl<S> WithHandshakeStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    fn new(io: S, acceptor: &TlsAcceptor) -> Self {
        WithHandshakeStream {
            state: State::MidHandshake(MidHandshake {
                inner: Some(acceptor.accept(io)),
            }),
        }
    }

    fn progress<T>(&mut self, f: impl FnOnce(&mut TlsStream<S>) -> io::Result<T>) -> io::Result<T> {
        loop {
            self.state = match &mut self.state {
                State::MidHandshake(m) => m.try_handshake().map(State::Ready)?,
                State::Ready(io) => return f(io),
                State::Gone => return Err(io::ErrorKind::Other.into()),
            };
        }
    }
}

impl<S> io::Read for WithHandshakeStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.progress(|io| io.read(buf))
    }
}

impl<S> io::Write for WithHandshakeStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.progress(|io| io.write(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        match &mut self.state {
            State::MidHandshake(m) => m.flush(),
            State::Ready(io) => io.flush(),
            State::Gone => Err(io::ErrorKind::Other.into()),
        }
    }
}

impl<S> AsyncRead for WithHandshakeStream<S> where S: AsyncRead + AsyncWrite {}

impl<S> AsyncWrite for WithHandshakeStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        match &mut self.state {
            State::MidHandshake(..) => {
                self.state = State::Gone;
                Ok(().into())
            }
            State::Ready(io) => {
                tokio_io::try_nb!(io.shutdown());
                io.get_mut().shutdown()
            }
            State::Gone => Ok(().into()),
        }
    }
}

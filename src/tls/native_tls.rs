#![cfg(feature = "native-tls")]

use {
    super::*,
    ::native_tls::{
        HandshakeError, //
        TlsAcceptor,
        TlsStream,
    },
    futures::{Async, Poll},
    std::io,
};

impl<T> TlsConfig<T> for TlsAcceptor
where
    T: AsyncRead + AsyncWrite,
{
    type Wrapped = NativeTlsStream<T>;
    type Wrapper = Self;

    #[inline]
    fn into_wrapper(self, _: Vec<String>) -> crate::Result<Self::Wrapper> {
        Ok(self)
    }
}

impl<T> TlsWrapper<T> for TlsAcceptor
where
    T: AsyncRead + AsyncWrite,
{
    type Wrapped = NativeTlsStream<T>;

    #[inline]
    fn wrap(&self, io: T) -> Self::Wrapped {
        NativeTlsStream {
            state: State::MidHandshake(MidHandshake {
                inner: Some(self.accept(io)),
            }),
        }
    }
}

#[derive(Debug)]
pub struct NativeTlsStream<S> {
    state: State<S>,
}

#[derive(Debug)]
enum State<S> {
    MidHandshake(MidHandshake<S>),
    Ready(TlsStream<S>),
    Gone,
}

impl<S> NativeTlsStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    pub fn is_ready(&self) -> bool {
        match self.state {
            State::Ready(..) => true,
            _ => false,
        }
    }

    pub fn poll_ready(&mut self) -> Poll<(), io::Error> {
        match self.with_handshake(|_| Ok(())) {
            Ok(()) => Ok(Async::Ready(())),
            Err(e) => {
                if e.kind() == io::ErrorKind::WouldBlock {
                    Ok(Async::NotReady)
                } else {
                    Err(e)
                }
            }
        }
    }

    fn with_handshake<T>(
        &mut self,
        f: impl FnOnce(&mut TlsStream<S>) -> io::Result<T>,
    ) -> io::Result<T> {
        loop {
            self.state = match &mut self.state {
                State::MidHandshake(m) => m.try_handshake().map(State::Ready)?,
                State::Ready(io) => return f(io),
                State::Gone => return Err(io::ErrorKind::Other.into()),
            };
        }
    }
}

impl<S> io::Read for NativeTlsStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.with_handshake(|io| io.read(buf))
    }
}

impl<S> io::Write for NativeTlsStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.with_handshake(|io| io.write(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        match &mut self.state {
            State::MidHandshake(m) => m.flush(),
            State::Ready(io) => io.flush(),
            State::Gone => Err(io::ErrorKind::Other.into()),
        }
    }
}

impl<S> AsyncRead for NativeTlsStream<S> where S: AsyncRead + AsyncWrite {}

impl<S> AsyncWrite for NativeTlsStream<S>
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

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
                Err(HandshakeError::Failure(err)) => Err(io::Error::new(io::ErrorKind::Other, err)),
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
                TlsStreamWithHandshake::MidHandshake(m) => m.try_handshake().map(Self::ready)?,
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
                TlsStreamWithHandshake::MidHandshake(m) => m.try_handshake().map(Self::ready)?,
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

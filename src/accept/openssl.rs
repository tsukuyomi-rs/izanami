#![cfg(feature = "openssl")]

use {
    super::*,
    ::openssl::ssl::{
        ErrorCode, //
        HandshakeError,
        MidHandshakeSslStream,
        ShutdownResult,
        SslAcceptor,
        SslStream,
    },
    futures::{Async, Poll},
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
                    if e.code() == ErrorCode::WANT_READ || e.code() == ErrorCode::WANT_WRITE =>
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

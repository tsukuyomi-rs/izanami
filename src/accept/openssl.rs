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
    type Accepted = WithHandshakeStream<T>;

    #[inline]
    fn accept(&self, io: T) -> Self::Accepted {
        WithHandshakeStream {
            state: State::MidHandshake(MidHandshake::Start {
                acceptor: self.clone(),
                io,
            }),
        }
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

    fn flush(&mut self) -> io::Result<()> {
        match self {
            MidHandshake::Start { io, .. } => io.flush(),
            MidHandshake::Handshake(s) => s.get_mut().flush(),
            MidHandshake::Done => Err(io::ErrorKind::Other.into()),
        }
    }

    fn shutdown(&mut self) -> Poll<(), io::Error> {
        match self {
            MidHandshake::Start { io, .. } => io.shutdown(),
            MidHandshake::Handshake(s) => s.get_mut().shutdown(),
            MidHandshake::Done => Err(io::ErrorKind::Other.into()),
        }
    }
}

#[allow(missing_debug_implementations)]
pub struct WithHandshakeStream<S> {
    state: State<S>,
}

#[allow(missing_debug_implementations)]
enum State<S> {
    MidHandshake(MidHandshake<S>),
    Ready(SslStream<S>),
    Gone,
}

impl<S> WithHandshakeStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    fn progress<T>(
        &mut self,
        op: impl FnOnce(&mut SslStream<S>) -> io::Result<T>,
    ) -> io::Result<T> {
        loop {
            self.state = match &mut self.state {
                State::MidHandshake(m) => match m.try_handshake()? {
                    Some(io) => State::Ready(io),
                    None => panic!("cannot perform handshake twice"),
                },
                State::Ready(io) => return op(io),
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

impl<S> AsyncRead for WithHandshakeStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    unsafe fn prepare_uninitialized_buffer(&self, _: &mut [u8]) -> bool {
        true
    }
}

impl<S> AsyncWrite for WithHandshakeStream<S>
where
    S: AsyncRead + AsyncWrite,
{
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        match &mut self.state {
            State::MidHandshake(m) => {
                futures::try_ready!(m.shutdown());
                self.state = State::Gone;
                Ok(Async::Ready(()))
            }
            State::Ready(io) => shutdown_io(io),
            State::Gone => Ok(().into()),
        }
    }
}

fn shutdown_io<S>(io: &mut SslStream<S>) -> Poll<(), io::Error>
where
    S: AsyncRead + AsyncWrite,
{
    match io.shutdown() {
        Ok(ShutdownResult::Sent) | Ok(ShutdownResult::Received) => Ok(Async::Ready(())),
        Err(ref e) if e.code() == ErrorCode::ZERO_RETURN => Ok(Async::Ready(())),
        Err(ref e) if e.code() == ErrorCode::WANT_READ || e.code() == ErrorCode::WANT_WRITE => {
            Ok(Async::NotReady)
        }
        Err(err) => Err({
            err.into_io_error()
                .unwrap_or_else(|e| io::Error::new(io::ErrorKind::Other, e))
        }),
    }
}

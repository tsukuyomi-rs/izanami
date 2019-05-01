//! WebSocket specific context values.

use {
    futures::{Async, AsyncSink, Poll, Sink, StartSend, Stream},
    std::fmt,
    tokio::io::{AsyncRead, AsyncWrite},
    tokio::sync::mpsc,
    tokio_tungstenite::WebSocketStream,
};

#[doc(inline)]
pub use tungstenite::{protocol::WebSocketConfig, Message};

#[derive(Debug)]
pub struct WebSocket {
    tx_send: mpsc::UnboundedSender<Message>,
    rx_recv: mpsc::UnboundedReceiver<Message>,
}

impl Stream for WebSocket {
    type Item = Message;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.rx_recv.poll().map_err(Error::recv)
    }
}

impl Sink for WebSocket {
    type SinkItem = Message;
    type SinkError = Error;

    fn start_send(&mut self, msg: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.tx_send.start_send(msg).map_err(Error::send)
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.tx_send.poll_complete().map_err(Error::send)
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.tx_send.close().map_err(Error::send)
    }
}

#[derive(Debug)]
pub(crate) struct WebSocketDriver {
    recv: Recv,
    send: Send,
}

impl WebSocketDriver {
    pub(crate) fn poll<I>(&mut self, socket: &mut WebSocketStream<I>) -> Poll<(), Error>
    where
        I: AsyncRead + AsyncWrite,
    {
        if let Async::Ready(()) = self.recv.poll_close(socket)? {
            self.send.rx.close();
            return Ok(Async::Ready(()));
        }

        self.send
            .poll(socket) //
            .map(|_| Async::NotReady)
    }
}

#[derive(Debug)]
struct Recv {
    tx: mpsc::UnboundedSender<Message>,
    buf: Option<Message>,
}

impl Recv {
    fn poll_close<I>(&mut self, socket: &mut WebSocketStream<I>) -> Poll<(), Error>
    where
        I: AsyncRead + AsyncWrite,
    {
        if let Some(msg) = self.buf.take() {
            futures::try_ready!(self.try_start_send(msg));
        }

        loop {
            match socket.poll().map_err(Error::protocol)? {
                Async::Ready(Some(msg)) => futures::try_ready!(self.try_start_send(msg)),
                Async::Ready(None) => return Ok(Async::Ready(())),
                Async::NotReady => {
                    futures::try_ready!(self.tx.poll_complete().map_err(Error::send));
                    return Ok(Async::NotReady);
                }
            }
        }
    }

    fn try_start_send(&mut self, msg: Message) -> Poll<(), Error> {
        debug_assert!(self.buf.is_none());
        match self.tx.start_send(msg).map_err(Error::send)? {
            AsyncSink::Ready => Ok(Async::Ready(())),
            AsyncSink::NotReady(msg) => {
                self.buf = Some(msg);
                Ok(Async::NotReady)
            }
        }
    }
}

#[derive(Debug)]
struct Send {
    rx: mpsc::UnboundedReceiver<Message>,
    buf: Option<Message>,
}

impl Send {
    fn poll<I>(&mut self, socket: &mut WebSocketStream<I>) -> Poll<(), Error>
    where
        I: AsyncRead + AsyncWrite,
    {
        if let Some(msg) = self.buf.take() {
            futures::try_ready!(self.try_start_send(socket, msg));
        }

        loop {
            match self.rx.poll().map_err(Error::recv)? {
                Async::Ready(Some(msg)) => futures::try_ready!(self.try_start_send(socket, msg)),
                Async::Ready(None) => {
                    futures::try_ready!(socket.close().map_err(Error::protocol));
                    return Ok(Async::Ready(()));
                }
                Async::NotReady => {
                    futures::try_ready!(socket.poll_complete().map_err(Error::protocol));
                    return Ok(Async::NotReady);
                }
            }
        }
    }

    fn try_start_send<I>(
        &mut self,
        socket: &mut WebSocketStream<I>,
        msg: Message,
    ) -> Poll<(), Error>
    where
        I: AsyncRead + AsyncWrite,
    {
        debug_assert!(self.buf.is_none());
        match socket.start_send(msg).map_err(Error::protocol)? {
            AsyncSink::Ready => Ok(Async::Ready(())),
            AsyncSink::NotReady(msg) => {
                self.buf = Some(msg);
                Ok(Async::NotReady)
            }
        }
    }
}

#[allow(missing_docs)]
#[derive(Debug)]
pub struct Error(ErrorKind);

#[derive(Debug)]
enum ErrorKind {
    Protocol(tungstenite::Error),
    Send(mpsc::error::UnboundedSendError),
    Recv(mpsc::error::UnboundedRecvError),
}

impl Error {
    fn protocol(err: tungstenite::Error) -> Self {
        Error(ErrorKind::Protocol(err))
    }

    fn recv(err: mpsc::error::UnboundedRecvError) -> Self {
        Error(ErrorKind::Recv(err))
    }

    fn send(err: mpsc::error::UnboundedSendError) -> Self {
        Error(ErrorKind::Send(err))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WebSocket error: ")?;
        match self.0 {
            ErrorKind::Protocol(ref e) => e.fmt(f),
            ErrorKind::Send(ref e) => e.fmt(f),
            ErrorKind::Recv(ref e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self.0 {
            ErrorKind::Protocol(ref e) => Some(e),
            ErrorKind::Send(ref e) => Some(e),
            ErrorKind::Recv(ref e) => Some(e),
        }
    }
}

#[allow(dead_code)]
pub(crate) fn pair() -> (WebSocket, WebSocketDriver) {
    let (tx_recv, rx_recv) = mpsc::unbounded_channel();
    let (tx_send, rx_send) = mpsc::unbounded_channel();

    let websocket = WebSocket { rx_recv, tx_send };
    let driver = WebSocketDriver {
        recv: Recv {
            tx: tx_recv,
            buf: None,
        },
        send: Send {
            rx: rx_send,
            buf: None,
        },
    };

    (websocket, driver)
}

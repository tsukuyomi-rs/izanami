use async_trait::async_trait;
use bytes::{Buf, Bytes};
use futures::future::poll_fn;
use h2::{
    server::{Connection, SendPushedResponse, SendResponse},
    RecvStream, SendStream,
};
use http::{HeaderMap, Request, Response};
use izanami::{App, Events, Message, PushEvents};
use std::{io, net::ToSocketAddrs};
use tokio::net::{TcpListener, TcpStream};

#[derive(Debug)]
pub struct Server {
    listener: TcpListener,
    h2: h2::server::Builder,
}

impl Server {
    pub async fn bind<A>(addr: A) -> io::Result<Self>
    where
        A: ToSocketAddrs,
    {
        let addr = addr.to_socket_addrs()?.next().unwrap();
        let listener = TcpListener::bind(&addr).await?;
        let h2 = h2::server::Builder::new();
        Ok(Self { listener, h2 })
    }

    pub async fn serve<T>(self, app: T) -> io::Result<()>
    where
        T: App + Clone + Send + Sync + 'static,
    {
        let mut listener = self.listener;
        loop {
            if let Ok((socket, _)) = listener.accept().await {
                let handshake = self.h2.handshake(socket);
                let app = app.clone();
                tokio::spawn(async move {
                    match handshake.await {
                        Ok(conn) => handle_connection(conn, app).await,
                        Err(err) => {
                            tracing::error!("handshake error: {}", err);
                            return;
                        }
                    }
                });
            }
        }
    }
}

async fn handle_connection<T>(mut conn: Connection<TcpStream, Bytes>, app: T)
where
    T: App + Clone + Send + Sync + 'static,
{
    loop {
        match conn.accept().await {
            Some(Ok((request, sender))) => {
                tokio::spawn(handle_request(app.clone(), request, sender));
            }
            Some(Err(err)) => {
                tracing::error!("accept error: {}", err);
                break;
            }
            None => {
                tracing::debug!("connection closed");
                break;
            }
        }
    }
}

async fn handle_request<T>(app: T, request: Request<RecvStream>, sender: SendResponse<Bytes>)
where
    T: App,
{
    let (parts, receiver) = request.into_parts();
    let request = Request::from_parts(parts, ());
    let mut events = H2Events {
        receiver,
        sender,
        stream: None,
    };

    if let Err(err) = app.call(&request, &mut events).await {
        tracing::error!("app error: {}", err);
    }
}

struct H2Events {
    receiver: RecvStream,
    sender: SendResponse<Bytes>,
    stream: Option<SendStream<Bytes>>,
}

#[async_trait]
impl Events for H2Events {
    type Data = io::Cursor<Bytes>;
    type Error = h2::Error;
    type PushEvents = H2PushEvents;

    async fn data(&mut self) -> Result<Option<Self::Data>, Self::Error> {
        let data = self.receiver.data().await.transpose()?;
        if let Some(ref data) = data {
            let release_capacity = self.receiver.release_capacity();
            release_capacity.release_capacity(data.len())?;
        }
        Ok(data.map(io::Cursor::new))
    }

    async fn trailers(&mut self) -> Result<Option<HeaderMap>, Self::Error> {
        self.receiver.trailers().await
    }

    async fn start_send_response(&mut self, response: Response<()>) -> Result<(), Self::Error> {
        let stream = self.sender.send_response(response, false)?;
        self.stream.replace(stream);
        Ok(())
    }

    async fn send_data<T>(&mut self, data: T, end_of_stream: bool) -> Result<(), Self::Error>
    where
        T: Buf + Send,
    {
        let stream = self.stream.as_mut().unwrap();

        stream.reserve_capacity(data.remaining());
        poll_fn(|cx| stream.poll_capacity(cx)).await.transpose()?;
        stream.send_data(data.collect(), end_of_stream)?;

        Ok(())
    }

    async fn send_trailers(&mut self, trailers: HeaderMap) -> Result<(), Self::Error> {
        let stream = self.stream.as_mut().unwrap();
        stream.send_trailers(trailers)
    }

    async fn push_request(
        &mut self,
        request: Request<()>,
    ) -> Result<Self::PushEvents, Self::Error> {
        Ok(H2PushEvents {
            sender: self.sender.push_request(request)?,
            stream: None,
        })
    }

    async fn start_websocket(&mut self, _: Response<()>) -> Result<(), Self::Error> {
        unimplemented!("Websocket over HTTP/2 is not supported")
    }

    async fn websocket_message(&mut self) -> Result<Option<Message>, Self::Error> {
        unimplemented!("WebSocket over HTTP/2 is not supported")
    }

    async fn send_websocket_message(&mut self, _: Message) -> Result<(), Self::Error> {
        unimplemented!("WebSocket over HTTP/2 is not supported")
    }
}

struct H2PushEvents {
    sender: SendPushedResponse<Bytes>,
    stream: Option<SendStream<Bytes>>,
}

#[async_trait]
impl PushEvents for H2PushEvents {
    type Error = h2::Error;

    async fn start_send_response(&mut self, response: Response<()>) -> Result<(), Self::Error> {
        let stream = self.sender.send_response(response, false)?;
        self.stream.replace(stream);
        Ok(())
    }

    async fn send_data<T>(&mut self, data: T, end_of_stream: bool) -> Result<(), Self::Error>
    where
        T: Buf + Send,
    {
        let stream = self.stream.as_mut().unwrap();

        stream.reserve_capacity(data.remaining());
        poll_fn(|cx| stream.poll_capacity(cx)).await.transpose()?;
        stream.send_data(data.collect(), end_of_stream)?;

        Ok(())
    }

    async fn send_trailers(&mut self, trailers: HeaderMap) -> Result<(), Self::Error> {
        let stream = self.stream.as_mut().unwrap();
        stream.send_trailers(trailers)
    }
}

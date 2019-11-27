use async_trait::async_trait;
use bytes::{Buf, Bytes};
use futures::future::poll_fn;
use h2::{
    server::{Connection, SendResponse},
    RecvStream, SendStream,
};
use http::{HeaderMap, Request, Response};
use izanami::App;
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
        T: for<'a> App<Events<'a>> + Clone + Send + Sync + 'static,
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

async fn handle_connection<T>(mut conn: Connection<TcpStream, Data>, app: T)
where
    T: for<'a> App<Events<'a>> + Clone + Send + Sync + 'static,
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

async fn handle_request<T>(app: T, request: Request<RecvStream>, mut sender: SendResponse<Data>)
where
    T: for<'a> App<Events<'a>>,
{
    let (parts, mut receiver) = request.into_parts();
    let mut stream = None;

    if let Err(err) = app
        .call(Request::from_parts(
            parts,
            Events {
                receiver: &mut receiver,
                sender: &mut sender,
                stream: &mut stream,
            },
        ))
        .await
    {
        let err = err.into();
        tracing::error!("app error: {}", err);
    }

    drop(receiver);
}

#[derive(Debug)]
pub struct Events<'a> {
    receiver: &'a mut RecvStream,
    sender: &'a mut SendResponse<Data>,
    stream: &'a mut Option<SendStream<Data>>,
}

impl Events<'_> {
    pub async fn data(&mut self) -> Option<Result<Data, h2::Error>> {
        let data = self.receiver.data().await;
        if let Some(Ok(ref data)) = data {
            let release_capacity = self.receiver.release_capacity();
            if let Err(err) = release_capacity.release_capacity(data.len()) {
                return Some(Err(err));
            }
        }
        data.map(|res| res.map(Data))
    }

    pub async fn trailers(&mut self) -> Result<Option<HeaderMap>, h2::Error> {
        self.receiver.trailers().await
    }

    pub async fn send_response<T>(&mut self, response: Response<T>) -> Result<(), h2::Error>
    where
        T: Into<Data>,
    {
        let (parts, body) = response.into_parts();
        let response = Response::from_parts(parts, ());
        self.start_send_response(response, false).await?;
        self.send_data(body.into(), true).await?;
        Ok(())
    }

    pub async fn start_send_response(
        &mut self,
        response: Response<()>,
        end_of_stream: bool,
    ) -> Result<(), h2::Error> {
        let stream = self.sender.send_response(response, end_of_stream)?;
        self.stream.replace(stream);
        Ok(())
    }

    pub async fn send_data<T>(&mut self, data: T, end_of_stream: bool) -> Result<(), h2::Error>
    where
        T: Into<Data>,
    {
        let stream = self.stream.as_mut().unwrap();
        let data = data.into();

        stream.reserve_capacity(data.remaining());
        poll_fn(|cx| stream.poll_capacity(cx)).await.transpose()?;
        stream.send_data(data, end_of_stream)?;

        Ok(())
    }

    pub async fn send_trailers(&mut self, trailers: HeaderMap) -> Result<(), h2::Error> {
        let stream = self.stream.as_mut().unwrap();
        stream.send_trailers(trailers)
    }
}

#[async_trait]
#[allow(clippy::needless_lifetimes)]
impl<'a> izanami::Events for Events<'a> {
    type Data = Data;
    type Error = h2::Error;

    #[inline]
    async fn data(&mut self) -> Option<Result<Self::Data, Self::Error>> {
        self.data().await
    }

    #[inline]
    async fn trailers(&mut self) -> Result<Option<HeaderMap>, Self::Error> {
        self.trailers().await
    }

    #[inline]
    async fn start_send_response(
        &mut self,
        response: Response<()>,
        end_of_stream: bool,
    ) -> Result<(), Self::Error> {
        self.start_send_response(response, end_of_stream).await
    }

    #[inline]
    async fn send_data(
        &mut self,
        data: Self::Data,
        end_of_stream: bool,
    ) -> Result<(), Self::Error> {
        self.send_data(data, end_of_stream).await
    }

    #[inline]
    async fn send_trailers(&mut self, trailers: HeaderMap) -> Result<(), Self::Error> {
        self.send_trailers(trailers).await
    }
}

#[derive(Debug)]
pub struct Data(Bytes);

impl<T: Into<Bytes>> From<T> for Data {
    fn from(bytes: T) -> Self {
        Self(bytes.into())
    }
}

impl Buf for Data {
    #[inline]
    fn remaining(&self) -> usize {
        self.0.len()
    }

    #[inline]
    fn bytes(&self) -> &[u8] {
        self.0.as_ref()
    }

    #[inline]
    fn advance(&mut self, amt: usize) {
        self.0.advance(amt);
    }
}

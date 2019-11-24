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
pub struct H2Server {
    listener: TcpListener,
    h2: h2::server::Builder,
}

impl H2Server {
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
        T: for<'a> App<H2Events<'a>> + Clone + Send + Sync + 'static,
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
    T: for<'a> App<H2Events<'a>> + Clone + Send + Sync + 'static,
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

async fn handle_request<T>(app: T, request: Request<RecvStream>, mut sender: SendResponse<Bytes>)
where
    T: for<'a> App<H2Events<'a>>,
{
    let (parts, mut receiver) = request.into_parts();
    let request = Request::from_parts(parts, ());
    let mut stream = None;

    if let Err(err) = app
        .call(
            &request,
            H2Events {
                receiver: &mut receiver,
                sender: &mut sender,
                stream: &mut stream,
            },
        )
        .await
    {
        tracing::error!("app error: {}", err);
    }
}

#[derive(Debug)]
pub struct H2Events<'a> {
    receiver: &'a mut RecvStream,
    sender: &'a mut SendResponse<Bytes>,
    stream: &'a mut Option<SendStream<Bytes>>,
}

impl H2Events<'_> {
    pub async fn data(&mut self) -> Result<Option<io::Cursor<Bytes>>, h2::Error> {
        let data = self.receiver.data().await.transpose()?;
        if let Some(ref data) = data {
            let release_capacity = self.receiver.release_capacity();
            release_capacity.release_capacity(data.len())?;
        }
        Ok(data.map(io::Cursor::new))
    }

    pub async fn trailers(&mut self) -> Result<Option<HeaderMap>, h2::Error> {
        self.receiver.trailers().await
    }

    pub async fn send_response<T>(&mut self, response: Response<T>) -> Result<(), h2::Error>
    where
        T: Buf + Send,
    {
        let (parts, body) = response.into_parts();
        let response = Response::from_parts(parts, ());
        self.start_send_response(response).await?;
        self.send_data(body, true).await?;
        Ok(())
    }

    pub async fn start_send_response(&mut self, response: Response<()>) -> Result<(), h2::Error> {
        let stream = self.sender.send_response(response, false)?;
        self.stream.replace(stream);
        Ok(())
    }

    pub async fn send_data<T>(&mut self, data: T, end_of_stream: bool) -> Result<(), h2::Error>
    where
        T: Buf + Send,
    {
        let stream = self.stream.as_mut().unwrap();

        stream.reserve_capacity(data.remaining());
        poll_fn(|cx| stream.poll_capacity(cx)).await.transpose()?;
        stream.send_data(data.collect(), end_of_stream)?;

        Ok(())
    }

    pub async fn send_trailers(&mut self, trailers: HeaderMap) -> Result<(), h2::Error> {
        let stream = self.stream.as_mut().unwrap();
        stream.send_trailers(trailers)
    }
}

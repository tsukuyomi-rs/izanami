use crate::app::{App, Eventer};
use async_trait::async_trait;
use bytes::Bytes;
use h2::{server::SendResponse, RecvStream};
use http::{Request, Response};
use std::io;
use std::net::ToSocketAddrs;
use tokio::net::TcpListener;

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
        let h2 = self.h2;
        loop {
            if let Ok((socket, _)) = listener.accept().await {
                let app = app.clone();
                let handshake = h2.handshake::<_, Bytes>(socket);
                tokio::spawn(async move {
                    match handshake.await {
                        Ok(mut conn) => {
                            while let Some(request) = conn.accept().await {
                                let (request, respond) = request.unwrap();
                                tokio::spawn(handle_request(app.clone(), request, respond));
                            }
                        }
                        Err(err) => {
                            eprintln!("handshake error: {}", err);
                        }
                    }
                });
            }
        }
    }
}

#[allow(unused_variables)]
async fn handle_request<T>(app: T, request: Request<RecvStream>, sender: SendResponse<Bytes>)
where
    T: App,
{
    let (parts, receiver) = request.into_parts();
    let request = Request::from_parts(parts, ());
    let mut eventer = H2Eventer { receiver, sender };

    if let Err(..) = app.call(&request, &mut eventer).await {
        eprintln!("App error");
    }
}

#[derive(Debug)]
pub struct H2Eventer {
    receiver: RecvStream,
    sender: SendResponse<Bytes>,
}

#[async_trait]
impl Eventer for H2Eventer {
    type Error = h2::Error;

    async fn send_response<T: AsRef<[u8]>>(
        &mut self,
        response: Response<T>,
    ) -> Result<(), Self::Error>
    where
        T: Send,
    {
        let (parts, body) = response.into_parts();
        let body = body.as_ref();
        let end_of_stream = body.is_empty();

        let mut stream = self
            .sender
            .send_response(Response::from_parts(parts, ()), end_of_stream)?;
        if !end_of_stream {
            stream.reserve_capacity(body.len());
            futures::future::poll_fn(|cx| stream.poll_capacity(cx))
                .await
                .transpose()?;
            stream.send_data(body.into(), true)?;
        }

        Ok(())
    }
}

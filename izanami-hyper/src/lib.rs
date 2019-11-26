use futures::{
    future::{poll_fn, Future},
    task::{self, Poll},
};
use http::{HeaderMap, Request, Response};
use http_body::Body as _Body;
use hyper::{
    body::{Body, Chunk, Sender as BodySender},
    server::{conn::AddrIncoming, Builder as ServerBuilder, Server},
    upgrade::Upgraded,
};
use izanami::{App, Events};
use std::{marker::PhantomData, net::ToSocketAddrs, pin::Pin};
use tokio::sync::oneshot;
use tower_service::Service;

#[derive(Debug)]
pub struct HyperServer {
    builder: ServerBuilder<AddrIncoming>,
}

impl HyperServer {
    pub async fn bind<A>(addr: A) -> hyper::Result<Self>
    where
        A: ToSocketAddrs,
    {
        let addr = addr.to_socket_addrs().unwrap().next().unwrap();
        Ok(Self {
            builder: Server::try_bind(&addr)?,
        })
    }

    pub async fn serve<T>(self, app: T) -> hyper::Result<()>
    where
        T: for<'a> App<HyperEvents<'a>> + Clone + Send + Sync + 'static,
    {
        let server = self
            .builder
            .serve(hyper::service::make_service_fn(move |_| {
                let app = app.clone();
                async move { Ok::<_, std::convert::Infallible>(AppService(app)) }
            }));
        server.await
    }
}

pub struct HyperEvents<'a> {
    req_body: Option<Body>,
    response_sender: Option<oneshot::Sender<Response<Body>>>,
    body_sender: Option<BodySender>,
    upgraded: Option<Upgraded>,
    _marker: PhantomData<&'a mut ()>,
}

impl Events for HyperEvents<'_> {}

impl HyperEvents<'_> {
    pub async fn data(&mut self) -> hyper::Result<Option<Chunk>> {
        let req_body = self.req_body.as_mut().unwrap();
        poll_fn(|cx| Pin::new(&mut *req_body).poll_data(cx))
            .await
            .transpose()
    }

    pub async fn trailers(&mut self) -> hyper::Result<Option<HeaderMap>> {
        let req_body = self.req_body.as_mut().unwrap();
        poll_fn(|cx| Pin::new(&mut *req_body).poll_trailers(cx)).await
    }

    pub async fn send_response<T>(&mut self, response: Response<T>) -> hyper::Result<()>
    where
        T: Into<Body>,
    {
        let sender = self.response_sender.take().unwrap();
        let _ = sender.send(response.map(Into::into));
        Ok(())
    }

    pub async fn start_send_response(&mut self, response: Response<()>) -> hyper::Result<()> {
        let sender = self.response_sender.take().unwrap();
        let (body_sender, body) = hyper::Body::channel();
        let _ = sender.send(response.map(|_| body));
        self.body_sender.replace(body_sender);
        Ok(())
    }

    pub async fn send_data<T>(&mut self, data: T, is_end_stream: bool) -> hyper::Result<()>
    where
        T: Into<Chunk>,
    {
        {
            let body_sender = self.body_sender.as_mut().unwrap();
            body_sender.send_data(data.into()).await?;
        }

        if is_end_stream {
            drop(self.body_sender.take());
        }

        Ok(())
    }

    pub async fn start_websocket(&mut self, response: Response<()>) -> hyper::Result<()> {
        let mut response = response;
        *response.status_mut() = http::StatusCode::SWITCHING_PROTOCOLS;

        let sender = self.response_sender.take().unwrap();
        let _ = sender.send(response.map(|_| Body::empty()));

        let req_body = self.req_body.take().unwrap();
        let upgraded = req_body.on_upgrade().await?;
        self.upgraded.replace(upgraded);

        Ok(())
    }

    pub fn as_upgraded(&mut self) -> Option<&mut Upgraded> {
        self.upgraded.as_mut()
    }
}

struct AppService<T>(T);

impl<T> AppService<T>
where
    T: for<'a> App<HyperEvents<'a>> + Clone + Send + Sync + 'static,
{
    fn spawn_background(&self, request: Request<Body>) -> oneshot::Receiver<Response<Body>> {
        let (parts, req_body) = request.into_parts();
        let request = Request::from_parts(parts, ());
        let app = self.0.clone();
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            if let Err(err) = app
                .call(
                    request,
                    HyperEvents {
                        req_body: Some(req_body),
                        response_sender: Some(tx),
                        body_sender: None,
                        upgraded: None,
                        _marker: PhantomData,
                    },
                )
                .await
            {
                eprintln!("app error: {}", err.into());
            }
        });
        rx
    }
}

impl<T> Service<Request<Body>> for AppService<T>
where
    T: for<'a> App<HyperEvents<'a>> + Clone + Send + Sync + 'static,
{
    type Response = Response<Body>;
    type Error = hyper::Error;
    #[allow(clippy::type_complexity)]
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn poll_ready(&mut self, _: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<hyper::Body>) -> Self::Future {
        let rx = self.spawn_background(request);
        Box::pin(async move { Ok(rx.await.unwrap()) })
    }
}

use futures::{
    future::{poll_fn, Future},
    task::{self, Poll},
};
use http::{HeaderMap, Request, Response, StatusCode};
use http_body::Body as _Body;
use hyper::{
    body::{Body, Chunk, Sender as BodySender},
    server::{conn::AddrIncoming, Builder as ServerBuilder, Server as HyperServer},
    upgrade::Upgraded,
};
use izanami::App;
use std::{marker::PhantomData, net::ToSocketAddrs, pin::Pin};
use tokio::sync::oneshot;
use tower_service::Service;

#[derive(Debug)]
pub struct Server {
    builder: ServerBuilder<AddrIncoming>,
}

impl Server {
    pub async fn bind<A>(addr: A) -> hyper::Result<Self>
    where
        A: ToSocketAddrs,
    {
        let addr = addr.to_socket_addrs().unwrap().next().unwrap();
        Ok(Self {
            builder: HyperServer::try_bind(&addr)?,
        })
    }

    pub async fn serve<T>(self, app: T) -> hyper::Result<()>
    where
        T: for<'a> App<Events<'a>> + Clone + Send + Sync + 'static,
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

#[derive(Debug)]
pub struct Events<'a> {
    req_body: Option<Body>,
    response_sender: Option<oneshot::Sender<Response<Body>>>,
    state: State,
    _marker: PhantomData<&'a mut ()>,
}

#[derive(Debug)]
enum State {
    Init,
    Streaming(BodySender),
    Upgraded(Upgraded),
    Done,
}

impl izanami::Events for Events<'_> {}

impl Events<'_> {
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

        if response.status() == StatusCode::SWITCHING_PROTOCOLS {
            let _ = sender.send(response.map(|_| Body::empty()));

            let req_body = self.req_body.take().unwrap();
            let upgraded = req_body.on_upgrade().await?;
            self.state = State::Upgraded(upgraded);
        } else {
            let (body_sender, body) = hyper::Body::channel();
            let _ = sender.send(response.map(|_| body));

            self.state = State::Streaming(body_sender);
        }
        Ok(())
    }

    pub async fn send_data<T>(&mut self, data: T, is_end_stream: bool) -> hyper::Result<()>
    where
        T: Into<Chunk>,
    {
        match &mut self.state {
            State::Streaming(sender) => {
                sender.send_data(data.into()).await?;
            }
            _ => panic!("unexpected call"),
        }

        if is_end_stream {
            self.state = State::Done;
        }

        Ok(())
    }
}

struct AppService<T>(T);

impl<T> AppService<T>
where
    T: for<'a> App<Events<'a>> + Clone + Send + Sync + 'static,
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
                    Events {
                        req_body: Some(req_body),
                        response_sender: Some(tx),
                        state: State::Init,
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
    T: for<'a> App<Events<'a>> + Clone + Send + Sync + 'static,
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

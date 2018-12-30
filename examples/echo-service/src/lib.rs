use {
    futures::{Async, Poll},
    http::{Request, Response},
    izanami_service::{http::BufStream, MakeService, Service},
};

#[derive(Debug)]
pub struct ResponseBody(Option<String>);

impl<T: Into<String>> From<T> for ResponseBody {
    fn from(data: T) -> Self {
        ResponseBody(Some(data.into()))
    }
}

impl BufStream for ResponseBody {
    type Item = std::io::Cursor<String>;
    type Error = std::io::Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        Ok(Async::Ready(self.0.take().map(std::io::Cursor::new)))
    }

    fn is_end_stream(&self) -> bool {
        self.0.is_none()
    }
}

#[derive(Debug, Default)]
pub struct Echo(());

impl<Ctx, Bd> MakeService<Ctx, Request<Bd>> for Echo {
    type Response = Response<ResponseBody>;
    type Error = std::io::Error;
    type Service = EchoService;
    type MakeError = std::io::Error;
    type Future = futures::future::FutureResult<Self::Service, Self::MakeError>;

    fn make_service(&self, _: Ctx) -> Self::Future {
        futures::future::ok(EchoService(()))
    }
}

#[derive(Debug)]
pub struct EchoService(());

impl<Bd> Service<Request<Bd>> for EchoService {
    type Response = Response<ResponseBody>;
    type Error = std::io::Error;
    type Future = futures::future::FutureResult<Self::Response, Self::Error>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, _: Request<Bd>) -> Self::Future {
        futures::future::ok(Response::new("hello, izanami".into()))
    }
}

use {
    http::{Request, Response},
    izanami_service::{MakeService, Service},
    std::io,
};

#[test]
fn version_sync() {
    version_sync::assert_html_root_url_updated!("src/lib.rs");
}

struct Echo;

impl<Ctx, Bd> MakeService<Ctx, Request<Bd>> for Echo {
    type Response = Response<String>;
    type Error = io::Error;
    type Service = Self;
    type MakeError = io::Error;
    type Future = futures::future::FutureResult<Self::Service, Self::MakeError>;

    fn make_service(&self, _: Ctx) -> Self::Future {
        futures::future::ok(Echo)
    }
}

impl<Bd> Service<Request<Bd>> for Echo {
    type Response = Response<String>;
    type Error = io::Error;
    type Future = futures::future::FutureResult<Self::Response, Self::Error>;

    fn poll_ready(&mut self) -> futures::Poll<(), Self::Error> {
        Ok(().into())
    }

    fn call(&mut self, _: Request<Bd>) -> Self::Future {
        futures::future::ok(Response::builder().body("hello".into()).unwrap())
    }
}

#[test]
fn threadpool_test_server() -> izanami_test::Result<()> {
    let mut server = izanami_test::server(Echo)?;

    let response = server.perform("/")?;
    assert_eq!(response.status(), 200);
    assert_eq!(response.body().to_utf8()?, "hello");

    Ok(())
}

#[test]
fn singlethread_test_server() -> izanami_test::Result<()> {
    let mut server = izanami_test::local_server(Echo)?;

    let response = server.perform("/")?;
    assert_eq!(response.status(), 200);
    assert_eq!(response.body().to_utf8()?, "hello");

    Ok(())
}

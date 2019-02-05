use {
    http::{Request, Response},
    izanami::test::TestServer,
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
fn test_server() -> izanami::Result<()> {
    let mut server = TestServer::new(Echo)?;

    let response = server
        .client()
        .request(
            Request::get("http://localhost/") //
                .body(hyper::Body::empty())
                .unwrap(),
        )
        .expect("client error");
    assert_eq!(response.status(), 200);

    server.shutdown()?;

    Ok(())
}

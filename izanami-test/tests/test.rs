use {
    http::{Request, Response},
    izanami_service::{MakeService, Service},
    izanami_test::Server,
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
    let mut server = Server::new(Echo)?;
    let mut client = server.client().build()?;

    let response = client.request(
        Request::get("/") //
            .body(())?,
    )?;
    assert_eq!(response.status(), 200);
    assert_eq!(response.send()?.to_utf8()?, "hello");

    Ok(())
}

#[test]
fn singlethread_test_server() -> izanami_test::Result<()> {
    let mut server = Server::new_current_thread(Echo)?;
    let mut client = server.client().build()?;

    let response = client.request(
        Request::get("/") //
            .body(())?,
    )?;
    assert_eq!(response.status(), 200);
    assert_eq!(response.send()?.to_utf8()?, "hello");

    Ok(())
}

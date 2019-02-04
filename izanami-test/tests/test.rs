use {
    futures::Future,
    http::{Request, Response},
    izanami_service::{MakeService, Service},
    izanami_test::{AsyncResult, Server},
    std::{
        io,
        time::{Duration, Instant},
    },
    tokio::timer::Delay,
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
fn threadpool_test_server() -> izanami_test::Result {
    izanami_test::with_default(|cx| {
        let mut server = Server::new(Echo);
        let mut client = server.client().wait(cx)?;

        let response = client
            .respond(
                Request::get("/") //
                    .body(())?,
            )
            .wait(cx)?;
        assert_eq!(response.status(), 200);

        let body = response.send_body().wait(cx)?;
        assert_eq!(body.to_utf8()?, "hello");

        Ok(())
    })
}

#[test]
fn singlethread_test_server() -> izanami_test::Result {
    izanami_test::with_current_thread(|cx| {
        let mut server = Server::new(Echo);
        let mut client = server.client().wait(cx)?;

        let response = client
            .respond(
                Request::get("/") //
                    .body(())?,
            )
            .wait(cx)?;
        assert_eq!(response.status(), 200);

        let body = response.send_body().wait(cx)?;
        assert_eq!(body.to_utf8()?, "hello");

        Ok(())
    })
}

#[test]
fn test_timeout() -> izanami_test::Result {
    struct EchoWithDelay;

    impl<Ctx, Bd> MakeService<Ctx, Request<Bd>> for EchoWithDelay {
        type Response = Response<String>;
        type Error = io::Error;
        type Service = Self;
        type MakeError = io::Error;
        type Future = futures::future::FutureResult<Self::Service, Self::MakeError>;

        fn make_service(&self, _: Ctx) -> Self::Future {
            futures::future::ok(EchoWithDelay)
        }
    }

    impl<Bd> Service<Request<Bd>> for EchoWithDelay {
        type Response = Response<String>;
        type Error = io::Error;
        type Future = Box<dyn Future<Item = Self::Response, Error = Self::Error> + Send + 'static>;

        fn poll_ready(&mut self) -> futures::Poll<(), Self::Error> {
            Ok(().into())
        }

        fn call(&mut self, _: Request<Bd>) -> Self::Future {
            Box::new(
                Delay::new(Instant::now() + Duration::from_secs(1))
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                    .map(|()| Response::builder().body("hello".into()).unwrap()),
            )
        }
    }

    izanami_test::with_default(|cx| {
        let mut server = Server::new(EchoWithDelay);

        let mut client = server.client().wait(cx)?;

        let result = client
            .respond(
                Request::get("/") //
                    .body(())?,
            )
            .wait(
                cx.duplicate() //
                    .timeout(Duration::from_millis(1)),
            );
        assert!(result.is_err());

        Ok(())
    })
}

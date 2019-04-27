use {
    crate::common::TestServer,
    futures::Future,
    http::{Request, Response},
    izanami_http::body::HttpBodyExt,
    izanami_server::protocol::h1::{H1Request, H1},
    std::io,
    tokio_buf::BufStreamExt,
};

#[test]
fn get() -> failure::Fallible<()> {
    let mut server = TestServer::start_h1(|stream| {
        H1::new().serve(
            stream,
            izanami_service::service_fn(|_req| {
                Response::builder()
                    .header("content-type", "text/plain")
                    .body("hello")
            }),
        )
    })?;

    let response = server.respond(
        Request::get("http://localhost/") //
            .body(hyper::Body::empty())?,
    )?;
    assert_eq!(response.status(), 200);
    assert_eq!(response.body(), "hello");

    server.shutdown();
    Ok(())
}

#[test]
fn post() -> failure::Fallible<()> {
    let mut server = TestServer::start_h1(|stream| {
        H1::new().serve(
            stream,
            izanami_service::service_fn(|req: H1Request| {
                req.into_body()
                    .into_buf_stream()
                    .collect::<Vec<u8>>()
                    .map_err(|_e| {
                        io::Error::new(io::ErrorKind::InvalidData, "failed to collect body")
                    })
                    .and_then(|body| {
                        Ok(Response::builder() //
                            .body(body)
                            .unwrap())
                    })
            }),
        )
    })?;

    let response = server.respond(
        Request::post("http://localhost/") //
            .body(hyper::Body::from("Ping"))?,
    )?;
    assert_eq!(response.status(), 200);
    assert_eq!(response.body(), "Ping");

    server.shutdown();
    Ok(())
}

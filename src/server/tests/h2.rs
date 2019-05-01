use {
    super::TestServer,
    crate::body::HttpBodyExt,
    crate::server::{
        protocol::h2::{H2Request, H2},
        service::service_fn,
    },
    futures::Future,
    http::{Request, Response},
    std::io,
    tokio_buf::BufStreamExt,
};

#[test]
fn get() -> failure::Fallible<()> {
    let mut server = TestServer::start_h2(|stream| {
        H2::new().serve(
            stream,
            service_fn(|_req| {
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
    let mut server = TestServer::start_h2(|stream| {
        H2::new().serve(
            stream,
            service_fn(|req: H2Request| {
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

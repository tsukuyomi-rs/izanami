use {
    http::Response,
    izanami::{
        server::{Incoming, Server}, //
        service::service_fn_ok,
    },
    std::io,
};

fn main() -> io::Result<()> {
    // A service factory called when a connection with the remote is established.
    let make_service = service_fn_ok(|()| {
        service_fn_ok(move |_req| {
            Response::builder()
                .header("content-type", "text/plain")
                .body("Hello")
                .unwrap()
        })
    });

    // A `Service` that produces a tuple of an HTTP service, a TCP stream
    // and an HTTP protocol configuration.
    let incoming_service = Incoming::bind_tcp("127.0.0.1:5000")? //
        .serve(make_service);

    // Starts an HTTP server using the above configuration.
    izanami::rt::run(Server::new(incoming_service));
    Ok(())
}

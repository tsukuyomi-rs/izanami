use {
    futures::prelude::*,
    http::Response,
    izanami::{
        server::{Incoming, Server}, //
        service::ServiceExt,
    },
    std::io,
};

#[allow(dead_code)]
struct ServiceContext {
    db_conn: (),
    // ...
}

impl ServiceContext {
    fn connect() -> impl Future<Item = Self, Error = io::Error> {
        futures::future::ok(ServiceContext { db_conn: () })
    }
}

fn main() -> io::Result<()> {
    // Since we want to build the value of service with the information
    // retrieved from incoming stream, so here returning just an empty service.
    let incoming_service = Incoming::bind_tcp("127.0.0.1:5000")? //
        .serve(izanami::service::service_fn(|()| ServiceContext::connect()));

    let incoming_service = incoming_service
        // First, wraps the service with `FixedService` to enable adaptor methods.
        .fixed_service()
        // Here, `incoming_service` is a `Service` that returns tuples of
        // the response of service specified for `serve`, TCP stream, and
        // the HTTP configuration used for serving them.
        .map(|(ctx, stream, protocol)| {
            // Extract the value of remote peer's address from stream.
            let remote_addr = stream.remote_addr();

            // If the stream uses SSL/TLS, the additional information
            // such as the selected ALPN protocol or SNI server name
            // are available at here.

            // Builds an HTTP service using the above values.
            let service = izanami::service::service_fn_ok(move |_req| {
                let _ = &ctx;
                eprintln!("remote_addr = {}", remote_addr);
                Response::builder()
                    .header("content-type", "text/plain")
                    .body("Hello")
                    .unwrap()
            });

            (service, stream, protocol)
        });

    let server = Server::new(incoming_service);
    izanami::rt::run(server);

    Ok(())
}

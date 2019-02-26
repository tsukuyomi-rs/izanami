use {
    http::Response,
    izanami::{
        net::tcp::AddrIncoming,
        server::Server, //
        service::{ext::ServiceExt, service_fn, stream::StreamExt},
    },
    std::io,
};

fn main() -> io::Result<()> {
    // A `Service` that produces a tuple of an HTTP service, a TCP stream
    // and an HTTP protocol configuration.
    let incoming_service = AddrIncoming::bind("127.0.0.1:5000")? //
        .into_service()
        .with_adaptors()
        .map(|stream| {
            let service = service_fn(move |_req| {
                Response::builder()
                    .header("content-type", "text/plain")
                    .body("Hello")
            });
            (stream, service)
        });

    // Starts an HTTP server using the above configuration.
    izanami::rt::run(Server::new(incoming_service));
    Ok(())
}

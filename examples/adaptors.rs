use {
    http::Response,
    izanami::{
        net::tcp::AddrIncoming,
        server::Server,
        service::{ext::ServiceExt, stream::StreamExt},
    },
    std::io,
};

fn main() -> io::Result<()> {
    let server = Server::new(
        AddrIncoming::bind("127.0.0.1:5000")? //
            .into_service()
            .with_adaptors()
            .map(|stream| {
                // Extract the value of remote peer's address from stream.
                let remote_addr = stream.remote_addr();

                // If the stream uses SSL/TLS, the additional information
                // such as the selected ALPN protocol or SNI server name
                // are available at here.

                // Builds an HTTP service using the above values.
                let service = izanami::service::service_fn(move |_req| {
                    eprintln!("remote_addr = {}", remote_addr);
                    Response::builder()
                        .header("content-type", "text/plain")
                        .body("Hello")
                });

                (stream, service)
            }),
    );

    izanami::rt::run(server);

    Ok(())
}

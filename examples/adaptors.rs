use {
    futures::Future,
    http::Response,
    izanami::{
        h1::H1Connection,
        net::tcp::AddrIncoming,
        server::Server,
        service::{ext::ServiceExt, service_fn},
    },
    std::io,
};

fn main() -> io::Result<()> {
    let server = Server::new(
        AddrIncoming::bind("127.0.0.1:5000")? //
            .service_map(|stream| {
                // Extract the value of remote peer's address from stream.
                let remote_addr = stream.remote_addr();

                // If the stream uses SSL/TLS, the additional information
                // such as the selected ALPN protocol or SNI server name
                // are available at here.

                H1Connection::build(stream) //
                    .finish(service_fn(move |_req| {
                        eprintln!("remote_addr = {}", remote_addr);
                        Response::builder()
                            .header("content-type", "text/plain")
                            .body("Hello")
                    }))
            }),
    )
    .map_err(|e| eprintln!("server error: {}", e));

    izanami::rt::run(server);

    Ok(())
}

use {
    futures::Future,
    http::Response,
    izanami::{
        net::tcp::AddrIncoming,
        server::{h1::H1Connection, Server}, //
        service::{ext::ServiceExt, service_fn, stream::StreamExt},
    },
    std::io,
};

fn main() -> io::Result<()> {
    let server = Server::new(
        AddrIncoming::bind("127.0.0.1:5000")? // Stream<Item = AddrStream>
            .into_service() // <-- Stream -> Service<()>
            .with_adaptors()
            .map(|stream| {
                let remote_addr = stream.remote_addr();
                H1Connection::builder(stream) //
                    .serve(service_fn(move |_req| -> io::Result<_> {
                        let _ = &remote_addr;
                        Ok(Response::builder()
                            .header("content-type", "text/plain")
                            .body("Hello")
                            .expect("valid response"))
                    }))
            }),
    )
    .map_err(|e| eprintln!("server error: {}", e));

    izanami::rt::run(server);
    Ok(())
}

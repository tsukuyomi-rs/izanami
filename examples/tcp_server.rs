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
    let server = Server::new(
        AddrIncoming::bind("127.0.0.1:5000")? // Stream<Item = AddrStream>
            .into_service() // <-- Stream -> Service<()>
            .with_adaptors()
            .map(|stream| {
                let remote_addr = stream.remote_addr();

                let service = service_fn(move |_req| -> io::Result<_> {
                    let _ = &remote_addr;
                    Ok(Response::builder()
                        .header("content-type", "text/plain")
                        .body("Hello")
                        .expect("valid response"))
                });

                (stream, service)
            }),
    );

    izanami::rt::run(server);
    Ok(())
}

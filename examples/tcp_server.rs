use {
    futures::Future,
    http::Response,
    izanami::{
        h1::H1Connection,
        net::tcp::AddrIncoming,
        server::Server, //
        service::{ext::ServiceExt, service_fn},
    },
    std::io,
};

fn main() -> io::Result<()> {
    let server = Server::new(
        AddrIncoming::bind("127.0.0.1:5000")? //
            .service_map(|stream| {
                let remote_addr = stream.remote_addr();
                H1Connection::build(stream) //
                    .finish(service_fn(move |_req| -> io::Result<_> {
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

#[cfg(unix)]
mod imp {
    use {
        http::Response,
        izanami::{
            net::unix::AddrIncoming,
            server::Server, //
            service::{ext::ServiceExt, service_fn, stream::StreamExt},
        },
        std::io,
    };

    pub fn main() -> io::Result<()> {
        let server = Server::new(
            AddrIncoming::bind("/tmp/echo-service.sock")? //
                .into_service()
                .with_adaptors()
                .map(|stream| {
                    let service = service_fn(|_req| {
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
}

#[cfg(not(unix))]
mod imp {
    pub fn main() -> std::io::Result<()> {
        println!("This example works only on Unix platform.");
        Ok(())
    }
}

fn main() -> std::io::Result<()> {
    crate::imp::main()
}

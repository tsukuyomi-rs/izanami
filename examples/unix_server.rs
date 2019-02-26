#[cfg(unix)]
mod imp {
    use {
        http::Response,
        izanami::{
            net::unix::AddrIncoming,
            server::Server, //
            service::{ext::ServiceExt, service_fn, service_fn_ok, stream::StreamExt},
        },
        std::io,
    };

    pub fn main() -> io::Result<()> {
        let incoming_service = AddrIncoming::bind("/tmp/echo-service.sock")? //
            .into_service()
            .with_adaptors()
            .with(service_fn_ok(|_| {
                service_fn(|_req| {
                    Response::builder()
                        .header("content-type", "text/plain")
                        .body("Hello")
                })
            }));

        izanami::rt::run(Server::new(incoming_service));
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

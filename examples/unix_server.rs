use {
    http::Response,
    izanami::{
        server::{Incoming, Server}, //
        service::service_fn_ok,
    },
    std::io,
};

#[cfg(unix)]
fn main() -> io::Result<()> {
    let incoming_service = Incoming::bind_unix("/tmp/echo-service.sock")? //
        .serve(service_fn_ok(|_| {
            service_fn_ok(|_req| {
                Response::builder()
                    .header("content-type", "text/plain")
                    .body("Hello")
                    .unwrap()
            })
        }));

    izanami::rt::run(Server::new(incoming_service));
    Ok(())
}

#[cfg(not(unix))]
fn main() {
    println!("This example works only on Unix platform.")
}

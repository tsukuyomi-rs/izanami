use {echo_service::Echo, http::Response};

#[cfg(unix)]
fn main() -> izanami::Result<()> {
    let echo = Echo::builder()
        .add_route("/", |_cx| {
            Response::builder() //
                .body("Hello")
                .unwrap()
        })?
        .build();

    let path = std::path::Path::new("/tmp/echo-service.sock");
    izanami::Server::bind(path).start(echo)
}

#[cfg(not(unix))]
fn main() {
    println!("This example works only on Unix platform.")
}
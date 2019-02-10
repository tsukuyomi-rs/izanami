use {
    echo_service::Echo, //
    http::Response,
    izanami::{Http, Server},
    std::path::Path,
};

#[cfg(unix)]
fn main() -> izanami::Result<()> {
    let echo = Echo::builder()
        .add_route("/", |_cx| {
            Response::builder() //
                .body("Hello")
                .unwrap()
        })?
        .build();

    let mut server = Server::default()?;
    server.spawn(
        Http::bind(Path::new("/tmp/echo-service.sock")) //
            .serve(echo)?,
    );
    server.run()
}

#[cfg(not(unix))]
fn main() {
    println!("This example works only on Unix platform.")
}

use {
    echo_service::Echo, //
    http::Response,
    izanami::Http,
    std::path::Path,
};

#[cfg(unix)]
fn main() -> izanami::Result<()> {
    izanami::system::default(|sys| {
        let echo = Echo::builder()
            .add_route("/", |_cx| {
                Response::builder() //
                    .body("Hello")
                    .unwrap()
            })?
            .build();

        sys.spawn(
            Http::bind(Path::new("/tmp/echo-service.sock")) //
                .serve(echo)?,
        );

        Ok(())
    })
}

#[cfg(not(unix))]
fn main() {
    println!("This example works only on Unix platform.")
}

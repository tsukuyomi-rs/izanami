use {
    echo_service::Echo, //
    http::Response,
    std::path::Path,
};

#[cfg(unix)]
fn main() -> izanami::Result<()> {
    izanami::system::run(|sys| {
        let echo = Echo::builder()
            .add_route("/", |_cx| {
                Response::builder() //
                    .body("Hello")
                    .unwrap()
            })?
            .build();

        sys.spawn(
            izanami::http::server(move || echo.clone()) //
                .bind(Path::new("/tmp/echo-service.sock")),
        );

        Ok(())
    })
}

#[cfg(not(unix))]
fn main() {
    println!("This example works only on Unix platform.")
}

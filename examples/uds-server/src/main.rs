use {
    echo_service::Echo, //
    http::Response,
    izanami::System,
    std::path::Path,
};

#[cfg(unix)]
fn main() -> izanami::Result<()> {
    System::with_default(|sys| {
        let echo = Echo::builder()
            .add_route("/", |_cx| {
                Response::builder() //
                    .body("Hello")
                    .unwrap()
            })?
            .build();

        izanami::http::server(move || echo.clone())
            .bind(Path::new("/tmp/echo-service.sock"))
            .start(sys);

        Ok(())
    })
}

#[cfg(not(unix))]
fn main() {
    println!("This example works only on Unix platform.")
}

use {
    echo_service::Echo, //
    http::Response,
    izanami::System,
};

fn main() -> izanami::Result<()> {
    System::with_default(move |sys| {
        let echo = Echo::builder()
            .add_route("/", |_cx| {
                Response::builder() //
                    .body("Hello")
                    .unwrap()
            })?
            .build();

        izanami::http::server(move || echo.clone()) //
            .bind("127.0.0.1:5000")
            .start(sys);

        Ok(())
    })
}

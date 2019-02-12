use {
    echo_service::Echo, //
    http::Response,
};

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
                .bind("127.0.0.1:5000"),
        );

        Ok(())
    })
}

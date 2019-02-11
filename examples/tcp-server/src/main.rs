use {
    echo_service::Echo, //
    http::Response,
    izanami::Http,
};

fn main() -> izanami::Result<()> {
    izanami::system::default(move |sys| {
        let echo = Echo::builder()
            .add_route("/", |_cx| {
                Response::builder() //
                    .body("Hello")
                    .unwrap()
            })?
            .build();

        sys.spawn(
            Http::bind("127.0.0.1:5000") //
                .serve(echo)?,
        );

        Ok(())
    })
}

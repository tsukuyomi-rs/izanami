use {echo_service::Echo, http::Response, std::net::SocketAddr};

fn main() -> izanami::Result<()> {
    let echo = Echo::builder()
        .add_route("/", |_cx| {
            Response::builder() //
                .body("Hello")
                .unwrap()
        })?
        .build();

    let addr = SocketAddr::from(([127, 0, 0, 1], 5000));
    izanami::Server::bind_tcp(&addr)? //
        .start(echo)
}

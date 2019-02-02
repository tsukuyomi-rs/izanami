use {
    echo_service::Echo,
    http::{Request, Response},
};

#[test]
fn test_empty_routes() -> izanami_test::Result<()> {
    let mut server = izanami_test::Server::new(
        Echo::builder() //
            .build(),
    )?;
    let mut client = server.client()?;

    let response = client.respond(
        Request::get("/") //
            .body(())?,
    )?;
    assert_eq!(response.status(), 404);
    Ok(())
}

#[test]
fn test_single_route() -> izanami_test::Result<()> {
    let mut server = izanami_test::Server::new(
        Echo::builder() //
            .add_route("/", |_| {
                Response::builder() //
                    .body("hello")
                    .unwrap()
            })?
            .build(),
    )?;
    let mut client = server.client()?;

    let response = client.respond(
        Request::get("/") //
            .body(())?,
    )?;
    assert_eq!(response.status(), 200);
    assert_eq!(response.send_body()?.to_utf8()?, "hello");

    Ok(())
}

#[test]
fn test_capture_param() -> izanami_test::Result<()> {
    let mut server = izanami_test::Server::new(
        Echo::builder() //
            .add_route("/([0-9]+)", |cx| {
                match cx
                    .captures()
                    .and_then(|c| c.get(1))
                    .and_then(|m| m.as_str().parse::<u32>().ok())
                {
                    Some(id) => Response::builder()
                        .status(200)
                        .body(format!("id={}", id))
                        .unwrap(),
                    None => Response::builder()
                        .status(400)
                        .body("missing or invalid id".into())
                        .unwrap(),
                }
            })?
            .build(),
    )?;
    let mut client = server.client()?;

    let response = client.respond(
        Request::get("/42") //
            .body(())?,
    )?;
    assert_eq!(response.status(), 200);
    assert_eq!(response.send_body()?.to_utf8()?, "id=42");

    let response = client.respond(
        Request::get("/fox") //
            .body(())?,
    )?;
    assert_eq!(response.status(), 404);

    Ok(())
}

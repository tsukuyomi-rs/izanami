use {echo_service::Echo, http::Response};

#[test]
fn test_empty_routes() -> izanami::Result<()> {
    let mut server = izanami::test::server(
        Echo::builder() //
            .build(),
    )?;

    assert_eq!(server.perform("/")?.status(), 404);

    Ok(())
}

#[test]
fn test_single_route() -> izanami::Result<()> {
    let mut server = izanami::test::server(
        Echo::builder() //
            .add_route("/", |_| {
                Response::builder() //
                    .body("hello")
                    .unwrap()
            })?
            .build(),
    )?;

    let response = server.perform("/")?;
    assert_eq!(response.status(), 200);
    assert_eq!(response.body().to_utf8()?, "hello");

    Ok(())
}

#[test]
fn test_capture_param() -> izanami::Result<()> {
    let mut server = izanami::test::server(
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

    let response = server.perform("/42")?;
    assert_eq!(response.status(), 200);
    assert_eq!(response.body().to_utf8()?, "id=42");

    let response = server.perform("/fox")?;
    assert_eq!(response.status(), 404);

    Ok(())
}

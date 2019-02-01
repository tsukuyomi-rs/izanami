use {crate::service::MockRequestBody, bytes::Bytes, http::Request};

/// A trait representing the input to the test server.
pub trait Input: InputImpl {}

pub trait InputImpl {
    fn build_request(self) -> http::Result<Request<MockRequestBody>>;
}

impl<T, E> Input for Result<T, E>
where
    T: Input,
    E: Into<http::Error>,
{
}

impl<T, E> InputImpl for Result<T, E>
where
    T: Input,
    E: Into<http::Error>,
{
    fn build_request(self) -> http::Result<Request<MockRequestBody>> {
        self.map_err(Into::into)?.build_request()
    }
}

impl Input for http::request::Builder {}

impl InputImpl for http::request::Builder {
    fn build_request(mut self) -> http::Result<Request<MockRequestBody>> {
        (&mut self).build_request()
    }
}

impl<'a> Input for &'a mut http::request::Builder {}

impl<'a> InputImpl for &'a mut http::request::Builder {
    fn build_request(self) -> http::Result<Request<MockRequestBody>> {
        self.body(MockRequestBody::new(Bytes::new()))
    }
}

impl Input for Request<()> {}

impl InputImpl for Request<()> {
    fn build_request(self) -> http::Result<Request<MockRequestBody>> {
        Ok(self.map(|_| MockRequestBody::new(Bytes::new())))
    }
}

impl<'a> Input for Request<&'a str> {}

impl<'a> InputImpl for Request<&'a str> {
    fn build_request(self) -> http::Result<Request<MockRequestBody>> {
        Ok(self.map(MockRequestBody::new))
    }
}

impl Input for Request<String> {}

impl InputImpl for Request<String> {
    fn build_request(self) -> http::Result<Request<MockRequestBody>> {
        Ok(self.map(MockRequestBody::new))
    }
}

impl<'a> Input for Request<&'a [u8]> {}

impl<'a> InputImpl for Request<&'a [u8]> {
    fn build_request(self) -> http::Result<Request<MockRequestBody>> {
        Ok(self.map(MockRequestBody::new))
    }
}

impl Input for Request<Vec<u8>> {}

impl InputImpl for Request<Vec<u8>> {
    fn build_request(self) -> http::Result<Request<MockRequestBody>> {
        Ok(self.map(MockRequestBody::new))
    }
}

impl Input for Request<Bytes> {}

impl InputImpl for Request<Bytes> {
    fn build_request(self) -> http::Result<Request<MockRequestBody>> {
        Ok(self.map(MockRequestBody::new))
    }
}

impl<'a> Input for &'a str {}

impl<'a> InputImpl for &'a str {
    fn build_request(self) -> http::Result<Request<MockRequestBody>> {
        Request::get(self) //
            .body(MockRequestBody::new(Bytes::new()))
    }
}

impl Input for String {}

impl InputImpl for String {
    fn build_request(self) -> http::Result<Request<MockRequestBody>> {
        self.as_str().build_request()
    }
}

#[cfg(test)]
mod test {
    use {super::*, http::Method};

    #[test]
    fn input_string() -> http::Result<()> {
        let request = "/foo".build_request()?;
        assert_eq!(request.method(), Method::GET);
        assert_eq!(request.uri().path(), "/foo");
        assert!(request.headers().is_empty());
        Ok(())
    }

    #[test]
    fn input_request_bytes() -> http::Result<()> {
        let request = Request::get("/") //
            .body(Bytes::new())
            .build_request()?;
        assert_eq!(request.method(), Method::GET);
        assert_eq!(request.uri().path(), "/");
        assert!(!request.headers().contains_key("content-type"));
        Ok(())
    }

    #[test]
    fn input_request_str() -> http::Result<()> {
        let request = Request::get("/") //
            .body("hello, izanami")
            .build_request()?;
        assert_eq!(request.method(), Method::GET);
        assert_eq!(request.uri().path(), "/");
        assert!(!request.headers().contains_key("content-type"));
        Ok(())
    }

    #[test]
    fn input_request_slice() -> http::Result<()> {
        let request = Request::new(&b""[..]).build_request()?;
        assert_eq!(request.method(), Method::GET);
        assert_eq!(request.uri().path(), "/");
        assert!(!request.headers().contains_key("content-type"));
        Ok(())
    }
}

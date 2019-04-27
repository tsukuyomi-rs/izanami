use {bytes::Buf, futures::Poll, izanami_http::HttpBody, izanami_util::MapAsyncOptExt, std::fmt};

trait BoxedHttpBody: Send + 'static {
    fn poll_data(&mut self) -> Poll<Option<Data>, Error>;
    fn poll_trailers(&mut self) -> Poll<Option<http::HeaderMap>, Error>;
    fn size_hint(&self) -> tokio_buf::SizeHint;
    fn is_end_stream(&self) -> bool;
    fn content_length(&self) -> Option<u64>;
}

impl<B> BoxedHttpBody for B
where
    B: HttpBody + Send + 'static,
    B::Data: Send + 'static,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    fn poll_data(&mut self) -> Poll<Option<Data>, Error> {
        HttpBody::poll_data(self)
            .map_async_opt(|data| Data(Box::new(data)))
            .map_err(|e| Error(e.into()))
    }

    fn poll_trailers(&mut self) -> Poll<Option<http::HeaderMap>, Error> {
        HttpBody::poll_trailers(self).map_err(|e| Error(e.into()))
    }

    fn size_hint(&self) -> tokio_buf::SizeHint {
        HttpBody::size_hint(self)
    }

    fn is_end_stream(&self) -> bool {
        HttpBody::is_end_stream(self)
    }

    fn content_length(&self) -> Option<u64> {
        HttpBody::content_length(self)
    }
}

pub struct Body(Box<dyn BoxedHttpBody>);

impl fmt::Debug for Body {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Body").finish()
    }
}

impl Body {
    pub fn new<B>(body: B) -> Self
    where
        B: HttpBody + Send + 'static,
        B::Data: Send + 'static,
        B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        Body(Box::new(body))
    }
}

impl HttpBody for Body {
    type Data = Data;
    type Error = Error;

    fn poll_data(&mut self) -> Poll<Option<Self::Data>, Self::Error> {
        self.0.poll_data()
    }

    fn poll_trailers(&mut self) -> Poll<Option<http::HeaderMap>, Self::Error> {
        self.0.poll_trailers()
    }

    fn size_hint(&self) -> tokio_buf::SizeHint {
        self.0.size_hint()
    }

    fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }

    fn content_length(&self) -> Option<u64> {
        self.0.content_length()
    }
}

pub struct Data(Box<dyn Buf + Send>);

impl fmt::Debug for Data {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Data").finish()
    }
}

impl Buf for Data {
    fn remaining(&self) -> usize {
        self.0.remaining()
    }

    fn bytes(&self) -> &[u8] {
        self.0.bytes()
    }

    fn advance(&mut self, amt: usize) {
        self.0.advance(amt)
    }
}

impl AsRef<[u8]> for Data {
    fn as_ref(&self) -> &[u8] {
        self.bytes()
    }
}

#[derive(Debug)]
pub struct Error(Box<dyn std::error::Error + Send + Sync>);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&*self.0, f)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

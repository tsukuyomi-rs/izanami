use {
    crate::size_hint::SizeHint,
    bytes::Buf,
    futures::{Async, Poll},
    std::{borrow::Cow, io},
};

/// A trait which abstracts an asynchronous stream of bytes.
pub trait BufStream {
    type Item: Buf;
    type Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error>;

    fn size_hint(&self) -> SizeHint {
        SizeHint::default()
    }
}

impl BufStream for String {
    type Item = io::Cursor<Vec<u8>>;
    type Error = io::Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.is_empty() {
            return Ok(Async::Ready(None));
        }

        let bytes = std::mem::replace(self, String::new()).into_bytes();
        Ok(Async::Ready(Some(io::Cursor::new(bytes))))
    }

    #[inline]
    fn size_hint(&self) -> SizeHint {
        SizeHint::exact(self.len() as u64)
    }
}

impl BufStream for Vec<u8> {
    type Item = io::Cursor<Vec<u8>>;
    type Error = io::Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.is_empty() {
            return Ok(Async::Ready(None));
        }

        let bytes = std::mem::replace(self, Vec::new());
        Ok(Async::Ready(Some(io::Cursor::new(bytes))))
    }

    #[inline]
    fn size_hint(&self) -> SizeHint {
        SizeHint::exact(self.len() as u64)
    }
}

impl BufStream for &'static str {
    type Item = io::Cursor<&'static [u8]>;
    type Error = io::Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.is_empty() {
            return Ok(Async::Ready(None));
        }

        let bytes = std::mem::replace(self, "").as_bytes();
        Ok(Async::Ready(Some(io::Cursor::new(bytes))))
    }

    #[inline]
    fn size_hint(&self) -> SizeHint {
        SizeHint::exact(self.len() as u64)
    }
}

impl BufStream for &'static [u8] {
    type Item = io::Cursor<&'static [u8]>;
    type Error = io::Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.is_empty() {
            return Ok(Async::Ready(None));
        }

        let bytes = std::mem::replace(self, &[]);
        Ok(Async::Ready(Some(io::Cursor::new(bytes))))
    }

    #[inline]
    fn size_hint(&self) -> SizeHint {
        SizeHint::exact(self.len() as u64)
    }
}

impl BufStream for Cow<'static, str> {
    type Item = io::Cursor<Cow<'static, [u8]>>;
    type Error = io::Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.is_empty() {
            return Ok(Async::Ready(None));
        }

        let bytes = match std::mem::replace(self, Cow::Borrowed("")) {
            Cow::Borrowed(borrowed) => Cow::Borrowed(borrowed.as_bytes()),
            Cow::Owned(owned) => Cow::Owned(owned.into_bytes()),
        };
        Ok(Async::Ready(Some(io::Cursor::new(bytes))))
    }

    #[inline]
    fn size_hint(&self) -> SizeHint {
        SizeHint::exact(self.len() as u64)
    }
}

impl BufStream for Cow<'static, [u8]> {
    type Item = io::Cursor<Cow<'static, [u8]>>;
    type Error = io::Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.is_empty() {
            return Ok(Async::Ready(None));
        }

        let bytes = std::mem::replace(self, Cow::Borrowed(&[]));
        Ok(Async::Ready(Some(io::Cursor::new(bytes))))
    }

    #[inline]
    fn size_hint(&self) -> SizeHint {
        SizeHint::exact(self.len() as u64)
    }
}

impl BufStream for bytes::Bytes {
    type Item = io::Cursor<bytes::Bytes>;
    type Error = io::Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.is_empty() {
            return Ok(Async::Ready(None));
        }

        let bytes = std::mem::replace(self, Default::default());
        Ok(Async::Ready(Some(io::Cursor::new(bytes))))
    }

    #[inline]
    fn size_hint(&self) -> SizeHint {
        SizeHint::exact(self.len() as u64)
    }
}

impl BufStream for bytes::BytesMut {
    type Item = io::Cursor<bytes::BytesMut>;
    type Error = io::Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.is_empty() {
            return Ok(Async::Ready(None));
        }

        let bytes = std::mem::replace(self, Default::default());
        Ok(Async::Ready(Some(io::Cursor::new(bytes))))
    }

    #[inline]
    fn size_hint(&self) -> SizeHint {
        SizeHint::exact(self.len() as u64)
    }
}

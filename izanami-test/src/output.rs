use {
    bytes::Bytes,
    std::{borrow::Cow, str},
};

/// A type representing a received HTTP message data from the server.
#[derive(Debug)]
pub struct Output {
    pub(super) chunks: Vec<Bytes>,
}

#[allow(missing_docs)]
impl Output {
    pub fn chunks(&self) -> &Vec<Bytes> {
        &self.chunks
    }

    pub fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(self.chunks().iter().fold(Vec::new(), |mut acc, chunk| {
            acc.extend_from_slice(&*chunk);
            acc
        }))
    }

    pub fn to_utf8(&self) -> Result<Cow<'_, str>, str::Utf8Error> {
        match self.to_bytes() {
            Cow::Borrowed(bytes) => str::from_utf8(bytes).map(Cow::Borrowed),
            Cow::Owned(bytes) => String::from_utf8(bytes)
                .map_err(|e| e.utf8_error())
                .map(Cow::Owned),
        }
    }
}

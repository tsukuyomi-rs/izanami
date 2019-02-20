use {
    crate::size_hint::SizeHint,
    bytes::{Buf, Bytes, BytesMut},
    std::{error::Error, fmt},
};

/// Trait representing the conversion from a `BufStream`.
pub trait FromBufStream<T: Buf>: Sized {
    type Builder;
    type Error;

    fn builder(hint: &SizeHint) -> Self::Builder;

    fn extend(builder: &mut Self::Builder, buf: &mut T, hint: &SizeHint)
        -> Result<(), Self::Error>;

    fn build(build: Self::Builder) -> Result<Self, Self::Error>;
}

#[derive(Debug)]
pub struct CollectVec {
    buf: Vec<u8>,
}

impl CollectVec {
    fn new(_: &SizeHint) -> Self {
        Self { buf: Vec::new() }
    }

    fn extend<T: Buf>(&mut self, buf: &mut T, _: &SizeHint) -> Result<(), CollectVecError> {
        self.buf.extend_from_slice(buf.bytes());
        Ok(())
    }

    fn build(self) -> Result<Vec<u8>, CollectVecError> {
        Ok(self.buf)
    }
}

#[derive(Debug)]
pub struct CollectVecError(());

impl fmt::Display for CollectVecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("collect vec error")
    }
}

impl Error for CollectVecError {}

impl<T> FromBufStream<T> for Vec<u8>
where
    T: Buf,
{
    type Builder = CollectVec;
    type Error = CollectVecError;

    #[inline]
    fn builder(hint: &SizeHint) -> Self::Builder {
        CollectVec::new(hint)
    }

    #[inline]
    fn extend(
        builder: &mut Self::Builder,
        buf: &mut T,
        hint: &SizeHint,
    ) -> Result<(), Self::Error> {
        builder.extend(buf, hint)
    }

    #[inline]
    fn build(builder: Self::Builder) -> Result<Self, Self::Error> {
        builder.build()
    }
}

#[derive(Debug)]
pub struct CollectBytes {
    buf: BytesMut,
}

impl CollectBytes {
    fn new(_hint: &SizeHint) -> Self {
        Self {
            buf: BytesMut::new(),
        }
    }

    fn extend<T: Buf>(&mut self, buf: &mut T, _: &SizeHint) -> Result<(), CollectBytesError> {
        self.buf.extend_from_slice(buf.bytes());
        Ok(())
    }

    fn build(self) -> Result<Bytes, CollectBytesError> {
        Ok(self.buf.freeze())
    }
}

#[derive(Debug)]
pub struct CollectBytesError(());

impl fmt::Display for CollectBytesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("collect bytes error")
    }
}

impl Error for CollectBytesError {}

impl<T> FromBufStream<T> for Bytes
where
    T: Buf,
{
    type Builder = CollectBytes;
    type Error = CollectBytesError;

    #[inline]
    fn builder(hint: &SizeHint) -> Self::Builder {
        CollectBytes::new(hint)
    }

    #[inline]
    fn extend(
        builder: &mut Self::Builder,
        buf: &mut T,
        hint: &SizeHint,
    ) -> Result<(), Self::Error> {
        builder.extend(buf, hint)
    }

    #[inline]
    fn build(builder: Self::Builder) -> Result<Self, Self::Error> {
        builder.build()
    }
}

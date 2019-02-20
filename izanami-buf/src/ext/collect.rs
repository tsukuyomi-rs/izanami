use {
    super::FromBufStream,
    crate::buf_stream::BufStream,
    futures::{Async, Future, Poll},
    std::{error::Error, fmt},
};

#[derive(Debug)]
pub struct Collect<S, T>
where
    S: BufStream,
    T: FromBufStream<S::Item>,
{
    pub(super) stream: S,
    pub(super) builder: Option<T::Builder>,
}

impl<S, T> Future for Collect<S, T>
where
    S: BufStream,
    T: FromBufStream<S::Item>,
{
    type Item = T;
    type Error = CollectError<S::Error, T::Error>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        while let Some(mut buf) = futures::try_ready! {
            self.stream.poll_buf()//
                .map_err(|e| CollectError {
                    kind: CollectErrorKind::Stream(e),
                })
        } {
            let builder = self.builder.as_mut().expect("cannot poll after done");
            T::extend(builder, &mut buf, &self.stream.size_hint()) //
                .map_err(|e| CollectError {
                    kind: CollectErrorKind::Collect(e),
                })?;
        }

        let builder = self.builder.take().expect("cannot poll after done");
        let value = T::build(builder) //
            .map_err(|e| CollectError {
                kind: CollectErrorKind::Collect(e),
            })?;

        Ok(Async::Ready(value))
    }
}

#[derive(Debug)]
pub struct CollectError<S, T> {
    kind: CollectErrorKind<S, T>,
}

#[derive(Debug)]
enum CollectErrorKind<S, T> {
    Stream(S),
    Collect(T),
}

impl<S, T> fmt::Display for CollectError<S, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("collect error")
    }
}

impl<S, T> Error for CollectError<S, T>
where
    S: Error + 'static,
    T: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            CollectErrorKind::Stream(e) => Some(e),
            CollectErrorKind::Collect(e) => Some(e),
        }
    }
}

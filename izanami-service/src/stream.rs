#![allow(missing_docs)]

use {
    crate::Service,
    futures::{Async, Poll, Stream},
};

type BoxedStdError = Box<dyn std::error::Error + Send + Sync + 'static>;

pub trait StreamExt: Stream {
    /// Wraps itself into `StreamService` to behave as a `Service`.
    ///
    /// The returned service polls a single value from the inner stream
    /// in `poll_ready` and returns its value as a future when invoking
    /// `call`.
    fn into_service(self) -> StreamService<Self>
    where
        Self: Sized,
        Self::Error: Into<BoxedStdError>,
    {
        StreamService {
            stream: self,
            state: State::Pending,
        }
    }
}

impl<T: Stream> StreamExt for T {}

/// A wrapper for `Stream`s that behaves as a `Service`.
#[derive(Debug)]
pub struct StreamService<T: Stream> {
    stream: T,
    state: State<T::Item>,
}

#[derive(Debug)]
enum State<I> {
    Pending,
    Ready(I),
}

impl<T: Stream> Service<()> for StreamService<T>
where
    T::Error: Into<BoxedStdError>,
{
    type Response = T::Item;
    type Error = BoxedStdError;
    type Future = futures::future::FutureResult<Self::Response, Self::Error>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        loop {
            self.state = match self.state {
                State::Pending => {
                    let item = futures::try_ready!(self.stream.poll().map_err(Into::into))
                        .ok_or_else(empty_stream)?;
                    State::Ready(item)
                }
                State::Ready(..) => return Ok(Async::Ready(())),
            };
        }
    }

    fn call(&mut self, _: ()) -> Self::Future {
        match std::mem::replace(&mut self.state, State::Pending) {
            State::Ready(item) => futures::future::ok(item),
            State::Pending => futures::future::err(empty_stream()),
        }
    }
}

fn empty_stream() -> BoxedStdError {
    use std::fmt;

    #[derive(Debug)]
    struct EmptyStream;

    impl fmt::Display for EmptyStream {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("empty stream")
        }
    }

    impl std::error::Error for EmptyStream {}

    EmptyStream.into()
}

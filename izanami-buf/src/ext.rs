mod collect;
mod from_buf_stream;

pub use self::{
    collect::{Collect, CollectError},
    from_buf_stream::FromBufStream,
};

use crate::buf_stream::BufStream;

pub trait BufStreamExt: BufStream {
    fn collect<T>(self) -> Collect<Self, T>
    where
        Self: Sized,
        T: FromBufStream<Self::Item>,
    {
        let builder = T::builder(&self.size_hint());
        Collect {
            stream: self,
            builder: Some(builder),
        }
    }
}

impl<T> BufStreamExt for T where T: BufStream {}

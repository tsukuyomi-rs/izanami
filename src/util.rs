use futures::Poll;

pub(crate) trait MapAsyncExt<T, E> {
    fn map_async<U>(self, op: impl FnOnce(T) -> U) -> Poll<U, E>;
}

impl<T, E> MapAsyncExt<T, E> for Poll<T, E> {
    fn map_async<U>(self, op: impl FnOnce(T) -> U) -> Poll<U, E> {
        self.map(|x| x.map(op))
    }
}

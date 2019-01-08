use futures::Poll;

pub trait ResultMapAsyncExt<T, E> {
    fn map_async<U>(self, f: impl FnOnce(T) -> U) -> Poll<U, E>;
}

impl<T, E> ResultMapAsyncExt<T, E> for Poll<T, E> {
    fn map_async<U>(self, f: impl FnOnce(T) -> U) -> Poll<U, E> {
        self.map(|x| x.map(f))
    }
}

pub trait ResultMapAsyncOptExt<T, E> {
    fn map_async_opt<U>(self, f: impl FnOnce(T) -> U) -> Poll<Option<U>, E>;
}

impl<T, E> ResultMapAsyncOptExt<T, E> for Poll<Option<T>, E> {
    fn map_async_opt<U>(self, f: impl FnOnce(T) -> U) -> Poll<Option<U>, E> {
        self.map(|x| x.map(|opt| opt.map(f)))
    }
}

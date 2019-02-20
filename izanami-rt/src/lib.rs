//! The abstraction of Tokio runtimes.

#![doc(html_root_url = "https://docs.rs/izanami-rt/0.1.0-preview.1")]
#![deny(
    missing_docs,
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]

#[doc(no_inline)]
pub use tokio_threadpool::{
    blocking as poll_blocking, //
    BlockingError,
};

use futures::Future;

/// Creates a `Future` to execute the specified function that will block the current thread.
///
/// The future genereted by this function internally calls the Tokio's blocking API,
/// and then enters a blocking section after other tasks are moved to another thread.
/// See [the documentation of `tokio_threadpool::blocking`][blocking] for details.
///
/// [blocking]: https://docs.rs/tokio-threadpool/0.1/tokio_threadpool/fn.blocking.html
pub fn blocking_section<F, T>(op: F) -> BlockingSection<F>
where
    F: FnOnce() -> T,
{
    BlockingSection { op: Some(op) }
}

#[allow(missing_docs)]
#[derive(Debug)]
pub struct BlockingSection<F> {
    op: Option<F>,
}

impl<F, T> Future for BlockingSection<F>
where
    F: FnOnce() -> T,
{
    type Item = T;
    type Error = BlockingError;

    #[inline]
    fn poll(&mut self) -> futures::Poll<Self::Item, Self::Error> {
        poll_blocking(|| {
            let op = self.op.take().expect("The future has already been polled");
            op()
        })
    }
}

/// A marker trait indicating that the implementor is a Tokio runtime.
pub trait Runtime: sealed::Runtime {}

impl Runtime for tokio::runtime::Runtime {}
impl Runtime for tokio::runtime::current_thread::Runtime {}

/// Trait representing the value that drives on the specific runtime
/// and returns a result.
pub trait Runnable<Rt>
where
    Rt: Runtime + ?Sized,
{
    /// The result type obtained by driving this value.
    type Output;

    /// Run this value onto the specified runtime until it completes.
    fn run(self, rt: &mut Rt) -> Self::Output;
}

impl<F> Runnable<tokio::runtime::Runtime> for F
where
    F: Future + Send + 'static,
    F::Item: Send + 'static,
    F::Error: Send + 'static,
{
    type Output = Result<F::Item, F::Error>;

    fn run(self, rt: &mut tokio::runtime::Runtime) -> Self::Output {
        rt.block_on(self)
    }
}

impl<F> Runnable<tokio::runtime::current_thread::Runtime> for F
where
    F: Future,
{
    type Output = Result<F::Item, F::Error>;

    fn run(self, rt: &mut tokio::runtime::current_thread::Runtime) -> Self::Output {
        rt.block_on(self)
    }
}

/// A marker trait indicating that the implementor is able to spawn asynchronous tasks.
pub trait Spawner: sealed::Spawner {}

impl Spawner for tokio::executor::DefaultExecutor {}
impl Spawner for tokio::runtime::Runtime {}
impl Spawner for tokio::runtime::TaskExecutor {}
impl Spawner for tokio::runtime::current_thread::Runtime {}
impl Spawner for tokio::runtime::current_thread::TaskExecutor {}

/// Trait representing the value to be spawned.
pub trait Spawn<Sp>
where
    Sp: Spawner + ?Sized,
{
    /// Spawns itself onto the specified spawner.
    fn spawn(self, spawner: &mut Sp);
}

impl<F> Spawn<tokio::runtime::Runtime> for F
where
    F: Future<Item = (), Error = ()> + Send + 'static,
{
    fn spawn(self, spawner: &mut tokio::runtime::Runtime) {
        spawner.spawn(self);
    }
}

impl<F> Spawn<tokio::runtime::current_thread::Runtime> for F
where
    F: Future<Item = (), Error = ()> + 'static,
{
    fn spawn(self, spawner: &mut tokio::runtime::current_thread::Runtime) {
        spawner.spawn(self);
    }
}

impl<F> Spawn<tokio::executor::DefaultExecutor> for F
where
    F: Future<Item = (), Error = ()> + Send + 'static,
{
    fn spawn(self, spawner: &mut tokio::executor::DefaultExecutor) {
        use tokio::executor::Executor;
        spawner
            .spawn(Box::new(self))
            .expect("failed to spawn the task");
    }
}

impl<F> Spawn<tokio::runtime::TaskExecutor> for F
where
    F: Future<Item = (), Error = ()> + Send + 'static,
{
    fn spawn(self, spawner: &mut tokio::runtime::TaskExecutor) {
        spawner.spawn(self);
    }
}

impl<F> Spawn<tokio::runtime::current_thread::TaskExecutor> for F
where
    F: Future<Item = (), Error = ()> + 'static,
{
    fn spawn(self, spawner: &mut tokio::runtime::current_thread::TaskExecutor) {
        spawner
            .spawn_local(Box::new(self))
            .expect("failed to spawn the task");
    }
}

mod sealed {
    pub trait Runtime {}
    impl Runtime for tokio::runtime::Runtime {}
    impl Runtime for tokio::runtime::current_thread::Runtime {}

    pub trait Spawner {}
    impl Spawner for tokio::executor::DefaultExecutor {}
    impl Spawner for tokio::runtime::Runtime {}
    impl Spawner for tokio::runtime::TaskExecutor {}
    impl Spawner for tokio::runtime::current_thread::Runtime {}
    impl Spawner for tokio::runtime::current_thread::TaskExecutor {}
}

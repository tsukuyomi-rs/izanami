//! The abstraction of Tokio runtimes.

use futures::Future;

/// Trait that abstracts the runtime for executing asynchronous tasks.
pub trait Runtime: self::sealed::Sealed {
    type Executor;

    fn executor(&self) -> Self::Executor;

    /// Spawn the specified future onto this runtime.
    fn spawn<F>(&mut self, future: F)
    where
        F: Spawn<Self>,
    {
        future.spawn(self)
    }

    /// Run the specified future onto this runtime and await its result.
    fn block_on<F>(&mut self, future: F) -> F::Output
    where
        F: Block<Self>,
    {
        future.block(self)
    }
}

impl Runtime for tokio::runtime::Runtime {
    type Executor = tokio::runtime::TaskExecutor;

    fn executor(&self) -> Self::Executor {
        self.executor()
    }
}

impl Runtime for tokio::runtime::current_thread::Runtime {
    type Executor = tokio::runtime::current_thread::TaskExecutor;

    fn executor(&self) -> Self::Executor {
        tokio::runtime::current_thread::TaskExecutor::current()
    }
}

/// Trait representing the value that drives on the specific runtime
/// and returns a result.
pub trait Block<Rt: Runtime + ?Sized> {
    /// The result type obtained by driving this value.
    type Output;

    /// Run this value onto the specified runtime until it completes.
    fn block(self, rt: &mut Rt) -> Self::Output;
}

impl<F> Block<tokio::runtime::Runtime> for F
where
    F: Future + Send + 'static,
    F::Item: Send + 'static,
    F::Error: Send + 'static,
{
    type Output = Result<F::Item, F::Error>;

    fn block(self, rt: &mut tokio::runtime::Runtime) -> Self::Output {
        rt.block_on(self)
    }
}

impl<F> Block<tokio::runtime::current_thread::Runtime> for F
where
    F: Future,
{
    type Output = Result<F::Item, F::Error>;

    fn block(self, rt: &mut tokio::runtime::current_thread::Runtime) -> Self::Output {
        rt.block_on(self)
    }
}

/// Trait representing the value to be spawned onto the specific runtime.
///
/// The role of this trait is similar to `Future`, but there are
/// the following differences:
///
/// * The implementor of this trait might spawns multiple tasks
///   with a single call.
/// * Unlike `Future<Item = (), Error= ()>`, this trait explicitly
///   specifies a reference to the runtime and can use its runtime
///   when constructing the task(s) to be spawned.
pub trait Spawn<Rt: Runtime + ?Sized> {
    /// Spawns itself onto the specified runtime.
    fn spawn(self, rt: &mut Rt);
}

impl<F> Spawn<tokio::runtime::Runtime> for F
where
    F: Future<Item = (), Error = ()> + Send + 'static,
{
    fn spawn(self, rt: &mut tokio::runtime::Runtime) {
        rt.spawn(self);
    }
}

impl<F> Spawn<tokio::runtime::current_thread::Runtime> for F
where
    F: Future<Item = (), Error = ()> + 'static,
{
    fn spawn(self, rt: &mut tokio::runtime::current_thread::Runtime) {
        rt.spawn(self);
    }
}

mod sealed {
    pub trait Sealed {}

    impl Sealed for tokio::runtime::Runtime {}
    impl Sealed for tokio::runtime::current_thread::Runtime {}
}

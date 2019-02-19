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

/// Trait representing the value to be spawned.
pub trait Spawn<Sp: ?Sized> {
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
    pub trait Sealed {}

    impl Sealed for tokio::runtime::Runtime {}
    impl Sealed for tokio::runtime::current_thread::Runtime {}
}

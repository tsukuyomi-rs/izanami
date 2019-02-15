//! The system for driving asynchronous computations.

use {futures::Future, std::marker::PhantomData};

/// Run the specified function onto the system using the default runtime.
pub fn default<F, T>(f: F) -> crate::Result<T>
where
    F: FnOnce(&mut System<'_>) -> crate::Result<T>,
{
    let mut sys = System::new(DefaultRuntime {
        inner: tokio::runtime::Runtime::new()?,
    });

    let ret = f(&mut sys)?;

    if sys.wait_incomplete_tasks {
        sys.runtime.inner.shutdown_on_idle().wait().unwrap();
    }

    Ok(ret)
}

/// Run the specified function onto the system using the single-threaded runtime.
pub fn current_thread<F, T>(f: F) -> crate::Result<T>
where
    F: FnOnce(&mut System<'_, CurrentThread>) -> crate::Result<T>,
{
    let mut sys = System::new(CurrentThread {
        inner: tokio::runtime::current_thread::Runtime::new()?,
    });

    let ret = f(&mut sys)?;

    if sys.wait_incomplete_tasks {
        sys.runtime.inner.run().unwrap();
    }

    Ok(ret)
}

/// A system that drives asynchronous computations.
#[derive(Debug)]
pub struct System<'s, Rt = DefaultRuntime>
where
    Rt: Runtime,
{
    runtime: Rt,
    wait_incomplete_tasks: bool,
    _anchor: PhantomData<&'s std::rc::Rc<()>>,
}

impl<'s, Rt> System<'s, Rt>
where
    Rt: Runtime,
{
    fn new(runtime: Rt) -> Self {
        Self {
            runtime,
            wait_incomplete_tasks: true,
            _anchor: PhantomData,
        }
    }

    /// Spawn the specified future onto this system.
    pub fn spawn<F>(&mut self, future: F)
    where
        F: Spawn<Rt>,
    {
        future.spawn(&mut self.runtime)
    }

    /// Run the specified future onto this system and await its result.
    pub fn block_on<F>(&mut self, future: F) -> F::Output
    where
        F: BlockOn<Rt>,
    {
        future.block_on(&mut self.runtime)
    }

    /// Specifies whether to wait for the completion of incomplete tasks
    /// before shutting down the runtime.
    ///
    /// The default value is `true`.
    pub fn wait_incomplete_tasks(&mut self, enabled: bool) {
        self.wait_incomplete_tasks = enabled;
    }
}

// ==== Runtime ====

/// Trait that abstracts the runtime for executing asynchronous tasks.
pub trait Runtime {}

#[derive(Debug)]
pub struct DefaultRuntime {
    inner: tokio::runtime::Runtime,
}

impl DefaultRuntime {
    pub(crate) fn executor(&self) -> tokio::runtime::TaskExecutor {
        self.inner.executor()
    }

    pub fn spawn<F>(&mut self, future: F)
    where
        F: Future<Item = (), Error = ()> + Send + 'static,
    {
        self.inner.spawn(future);
    }

    pub fn block_on<F>(&mut self, future: F) -> Result<F::Item, F::Error>
    where
        F: Future + Send + 'static,
        F::Item: Send + 'static,
        F::Error: Send + 'static,
    {
        self.inner.block_on(future)
    }
}

impl Runtime for DefaultRuntime {}

#[derive(Debug)]
pub struct CurrentThread {
    inner: tokio::runtime::current_thread::Runtime,
}

impl CurrentThread {
    pub(crate) fn executor(&self) -> tokio::runtime::current_thread::TaskExecutor {
        tokio::runtime::current_thread::TaskExecutor::current()
    }

    pub fn spawn<F>(&mut self, future: F)
    where
        F: Future<Item = (), Error = ()> + 'static,
    {
        self.inner.spawn(future);
    }

    pub fn block_on<F>(&mut self, future: F) -> Result<F::Item, F::Error>
    where
        F: Future,
    {
        self.inner.block_on(future)
    }
}

impl Runtime for CurrentThread {}

/// Trait representing the value that drives on the specific runtime
/// and returns a result.
pub trait BlockOn<Rt: Runtime + ?Sized = DefaultRuntime> {
    /// The result type obtained by driving this value.
    type Output;

    /// Run this value onto the specified runtime until it completes.
    fn block_on(self, rt: &mut Rt) -> Self::Output;
}

impl<F> BlockOn<DefaultRuntime> for F
where
    F: Future + Send + 'static,
    F::Item: Send + 'static,
    F::Error: Send + 'static,
{
    type Output = Result<F::Item, F::Error>;

    fn block_on(self, rt: &mut DefaultRuntime) -> Self::Output {
        rt.block_on(self)
    }
}

impl<F> BlockOn<CurrentThread> for F
where
    F: Future,
{
    type Output = Result<F::Item, F::Error>;

    fn block_on(self, rt: &mut CurrentThread) -> Self::Output {
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
pub trait Spawn<Rt: Runtime + ?Sized = DefaultRuntime> {
    /// Spawns itself onto the specified runtime.
    fn spawn(self, rt: &mut Rt);
}

impl<F> Spawn<DefaultRuntime> for F
where
    F: Future<Item = (), Error = ()> + Send + 'static,
{
    fn spawn(self, rt: &mut DefaultRuntime) {
        rt.spawn(self);
    }
}

impl<F> Spawn<CurrentThread> for F
where
    F: Future<Item = (), Error = ()> + 'static,
{
    fn spawn(self, rt: &mut CurrentThread) {
        rt.spawn(self);
    }
}

//! The system for driving asynchronous computations.

use {futures::Future, std::marker::PhantomData};

/// Run the specified function onto the system using the default runtime.
pub fn run<F, T>(f: F) -> crate::Result<T>
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
pub fn run_local<F, T>(f: F) -> crate::Result<T>
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

    /// Spawn the specified future onto this system and returns the associated `Handle`.
    pub fn spawn<F>(&mut self, future: F) -> Handle<'s, Rt, F::Output>
    where
        F: Spawn<Rt>,
    {
        Handle {
            rx_complete: future.spawn(&mut self.runtime),
            _marker: PhantomData,
        }
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
        self.wait_incomplete_tasks = enabled
    }
}

/// The handle for managing a spawned task.
#[derive(Debug)]
pub struct Handle<'rt, Rt: Runtime, T> {
    rx_complete: notify::Receiver<T>,
    _marker: PhantomData<fn(&mut System<'rt, Rt>)>,
}

impl<'rt, Rt, T> Handle<'rt, Rt, T>
where
    Rt: Runtime,
{
    /// Wait for the completion of associated task and returns its result.
    pub fn wait_complete(self, sys: &mut System<'rt, Rt>) -> T
    where
        notify::Receiver<T>: BlockOn<Rt, Output = Result<T, ()>>,
    {
        sys.block_on(self.rx_complete) //
            .unwrap_or_else(|_| panic!("failed to receive completion signal"))
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

/// Trait representing the ability to run a future.
pub trait BlockOn<Rt: Runtime = DefaultRuntime> {
    type Output;

    /// Run the provided future onto this runtime until it completes.
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

/// Trait representing the value to be spawned onto `System`.
///
/// The role of this trait is similar to `Future`, but it explicitly
/// specifies the type of runtime to be spawned.
pub trait Spawn<Rt: Runtime = DefaultRuntime> {
    /// The output type which will be returned when the spawned task completes.
    type Output;

    /// Spawned itself onto the specified system.
    fn spawn(self, rt: &mut Rt) -> notify::Receiver<Self::Output>;
}

impl<F> Spawn<DefaultRuntime> for F
where
    F: Future + Send + 'static,
    F::Item: Send + 'static,
    F::Error: Send + 'static,
{
    type Output = Result<F::Item, F::Error>;

    fn spawn(self, rt: &mut DefaultRuntime) -> notify::Receiver<Self::Output> {
        let (tx, rx) = notify::pair();
        rt.spawn(self.then(move |result| {
            tx.send(result);
            Ok(())
        }));
        rx
    }
}

impl<F> Spawn<CurrentThread> for F
where
    F: Future + 'static,
{
    type Output = Result<F::Item, F::Error>;

    fn spawn(self, rt: &mut CurrentThread) -> notify::Receiver<Self::Output> {
        let (tx, rx) = notify::pair();
        rt.spawn(self.then(move |result| {
            tx.send(result);
            Ok(())
        }));
        rx
    }
}

#[doc(hidden)]
pub mod notify {
    use {
        futures::{Future, Poll},
        tokio::sync::oneshot,
    };

    pub(crate) fn pair<T>() -> (Sender<T>, Receiver<T>) {
        let (tx, rx) = oneshot::channel();
        (Sender { inner: tx }, Receiver { inner: Ok(rx) })
    }

    #[derive(Debug)]
    pub(crate) struct Sender<T> {
        inner: oneshot::Sender<T>,
    }

    impl<T> Sender<T> {
        pub(crate) fn send(self, value: T) {
            let _ = self.inner.send(value);
        }
    }

    #[derive(Debug)]
    pub struct Receiver<T> {
        inner: Result<oneshot::Receiver<T>, Option<T>>,
    }

    impl<T> Future for Receiver<T> {
        type Item = T;
        type Error = ();

        #[inline]
        fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
            match &mut self.inner {
                Ok(rx) => rx.poll().map_err(|_| ()),
                Err(value) => Ok(value
                    .take()
                    .expect("the future has already been polled")
                    .into()),
            }
        }
    }
}

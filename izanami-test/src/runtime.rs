use {
    crate::error::BoxedStdError,
    futures::Future,
    std::panic::{resume_unwind, AssertUnwindSafe},
};

pub trait Awaitable<Rt: ?Sized> {
    type Ok;
    type Error;

    fn wait(self, rt: &mut Rt) -> Result<Self::Ok, Self::Error>;
}

/// Start the default runtime and run the specified closure onto this runtime.
pub fn with_default<T>(
    f: impl FnOnce(&mut DefaultRuntime) -> crate::Result<T>,
) -> crate::Result<T> {
    let mut runtime = DefaultRuntime::new()?;
    let ret = f(&mut runtime)?;
    runtime.shutdown();
    Ok(ret)
}

/// Start the current thread runtime and run the specified closure onto this runtime.
pub fn with_current_thread<T>(
    f: impl FnOnce(&mut CurrentThread) -> crate::Result<T>,
) -> crate::Result<T> {
    let mut runtime = CurrentThread::new()?;
    let ret = f(&mut runtime)?;
    runtime.shutdown();
    Ok(ret)
}

/// A trait that abstracts the runtime for driving a `Future`.
pub trait Runtime<F: Future>
where
    F: Future,
    F::Error: Into<BoxedStdError>,
{
    /// Run a `Future` to completion on this runtime.
    fn block_on(&mut self, future: F) -> crate::Result<F::Item>;
}

/// An implementor of `Runtime<S>` using the default Tokio runtime.
#[derive(Debug)]
pub struct DefaultRuntime {
    runtime: tokio::runtime::Runtime,
}

impl DefaultRuntime {
    fn new() -> crate::Result<Self> {
        let mut builder = tokio::runtime::Builder::new();
        builder.core_threads(1);
        builder.blocking_threads(1);
        builder.name_prefix("izanami-test");

        Ok(Self {
            runtime: builder.build()?,
        })
    }

    fn block_on<F>(&mut self, mut future: F) -> crate::Result<F::Item>
    where
        F: Future + Send + 'static,
        F::Item: Send + 'static,
        F::Error: Into<BoxedStdError>,
    {
        let future = futures::future::poll_fn(move || {
            future.poll().map_err(crate::Error::from_boxed_compat)
        });
        match self
            .runtime
            .block_on(AssertUnwindSafe(future).catch_unwind())
        {
            Ok(result) => result,
            Err(err) => resume_unwind(Box::new(err)),
        }
    }

    fn shutdown(self) {
        self.runtime.shutdown_on_idle().wait().unwrap();
    }
}

impl<F> Runtime<F> for DefaultRuntime
where
    F: Future + Send + 'static,
    F::Item: Send + 'static,
    F::Error: Into<BoxedStdError>,
{
    fn block_on(&mut self, future: F) -> crate::Result<F::Item> {
        self.block_on(future)
    }
}

/// An implementor of `Runtime<S>` using single threaded Tokio runtime.
#[derive(Debug)]
pub struct CurrentThread {
    runtime: tokio::runtime::current_thread::Runtime,
}

impl CurrentThread {
    fn new() -> crate::Result<Self> {
        Ok(Self {
            runtime: tokio::runtime::current_thread::Runtime::new()?,
        })
    }

    fn block_on<F>(&mut self, mut future: F) -> crate::Result<F::Item>
    where
        F: Future,
        F::Error: Into<BoxedStdError>,
    {
        self.runtime.block_on(futures::future::poll_fn(move || {
            future.poll().map_err(crate::Error::from_boxed_compat)
        }))
    }

    fn shutdown(mut self) {
        self.runtime.run().unwrap();
    }
}

impl<F> Runtime<F> for CurrentThread
where
    F: Future,
    F::Error: Into<BoxedStdError>,
{
    fn block_on(&mut self, future: F) -> crate::Result<F::Item> {
        self.block_on(future)
    }
}

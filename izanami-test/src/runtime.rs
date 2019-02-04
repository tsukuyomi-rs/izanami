use {
    crate::{context::Context, error::BoxedStdError},
    futures::future::{poll_fn, Either, Future},
    std::{
        panic::{resume_unwind, AssertUnwindSafe},
        time::Duration,
    },
    tokio::timer::Timeout,
};

/// Start the default runtime and run the specified closure onto this runtime.
pub fn with_default<F, R>(f: F) -> crate::Result<R>
where
    F: FnOnce(&mut Context<'_>) -> crate::Result<R>,
{
    let mut runtime = {
        let mut builder = tokio::runtime::Builder::new();
        builder.core_threads(1);
        builder.blocking_threads(1);
        builder.name_prefix("izanami-test");
        builder.build()?
    };

    let ret = f(&mut Context::new(&mut runtime))?;

    runtime.shutdown_on_idle().wait().unwrap();

    Ok(ret)
}

/// A trait that abstracts the runtime for driving a `Future`.
pub trait Runtime<F>
where
    F: Future,
    F::Error: Into<BoxedStdError>,
{
    /// Run a `Future` to completion on this runtime.
    fn block_on(&mut self, future: F, timeout: Option<Duration>) -> crate::Result<F::Item>;
}

impl<F> Runtime<F> for tokio::runtime::Runtime
where
    F: Future + Send + 'static,
    F::Item: Send + 'static,
    F::Error: Into<BoxedStdError>,
{
    fn block_on(&mut self, future: F, timeout: Option<Duration>) -> crate::Result<F::Item> {
        let future = maybe_timeout(future, timeout);
        match self.block_on(AssertUnwindSafe(future).catch_unwind()) {
            Ok(result) => result,
            Err(err) => resume_unwind(Box::new(err)),
        }
    }
}

// ==== CurrentThread ====

/// Start the current thread runtime and run the specified closure onto this runtime.
pub fn with_current_thread<F, R>(f: F) -> crate::Result<R>
where
    F: FnOnce(&mut Context<'_, tokio::runtime::current_thread::Runtime>) -> crate::Result<R>,
{
    let mut runtime = {
        let mut builder = tokio::runtime::current_thread::Builder::new();
        builder.build()?
    };

    let ret = f(&mut Context::new(&mut runtime))?;

    runtime.run().unwrap();

    Ok(ret)
}

impl<F> Runtime<F> for tokio::runtime::current_thread::Runtime
where
    F: Future,
    F::Error: Into<BoxedStdError>,
{
    fn block_on(&mut self, future: F, timeout: Option<Duration>) -> crate::Result<F::Item> {
        let future = maybe_timeout(future, timeout);
        self.block_on(future)
    }
}

fn maybe_timeout<F>(
    mut future: F,
    timeout: Option<Duration>,
) -> impl Future<Item = F::Item, Error = crate::Error>
where
    F: Future,
    F::Error: Into<BoxedStdError>,
{
    if let Some(timeout) = timeout {
        let mut future = Timeout::new(future, timeout);
        Either::A(poll_fn(move || {
            future.poll().map_err(|err| {
                if err.is_inner() {
                    crate::Error::from_boxed_compat(err.into_inner().unwrap())
                } else if err.is_elapsed() {
                    crate::Error::from(failure::format_err!("timeout"))
                } else {
                    crate::Error::from(err.into_timer().unwrap())
                }
            })
        }))
    } else {
        Either::B(poll_fn(move || {
            future.poll().map_err(crate::Error::from_boxed_compat)
        }))
    }
}

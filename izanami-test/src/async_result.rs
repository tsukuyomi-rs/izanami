use {
    crate::{context::Context, error::BoxedStdError, runtime::Runtime},
    futures::Future,
};

pub trait AsyncResult<Rt: ?Sized> {
    type Output;

    fn wait(self, cx: &mut Context<'_, Rt>) -> crate::Result<Self::Output>;
}

impl<F, Rt: ?Sized> AsyncResult<Rt> for F
where
    F: Future,
    F::Error: Into<BoxedStdError>,
    Rt: Runtime<F>,
{
    type Output = F::Item;

    fn wait(self, cx: &mut Context<'_, Rt>) -> crate::Result<Self::Output> {
        cx.block_on(self)
    }
}

pub(crate) fn wait_fn<F, Rt, T>(op: F) -> impl AsyncResult<Rt, Output = T>
where
    F: FnOnce(&mut Context<'_, Rt>) -> crate::Result<T>,
{
    #[allow(missing_debug_implementations)]
    struct AsyncResultFn<F>(F);

    impl<F, Rt, T> AsyncResult<Rt> for AsyncResultFn<F>
    where
        F: FnOnce(&mut Context<'_, Rt>) -> crate::Result<T>,
    {
        type Output = T;

        fn wait(self, cx: &mut Context<'_, Rt>) -> crate::Result<Self::Output> {
            (self.0)(cx)
        }
    }

    AsyncResultFn(op)
}

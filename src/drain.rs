use {
    futures::{future::Shared, Async, Future, Poll, Stream},
    tokio::sync::{mpsc, oneshot},
};

// FIXME: replace with never type.
enum Never {}

pub(crate) fn channel() -> (Signal, Watch) {
    let (tx, rx) = oneshot::channel();
    let (tx_drained, rx_drained) = mpsc::channel(1);
    (
        Signal { tx, rx_drained },
        Watch {
            rx: rx.shared(),
            tx_drained,
        },
    )
}

#[derive(Debug)]
pub(crate) struct Signal {
    tx: oneshot::Sender<()>,
    rx_drained: mpsc::Receiver<Never>,
}

impl Signal {
    pub(crate) fn drain(self) -> Draining {
        Draining {
            rx_drained: self.rx_drained,
        }
    }
}

impl Future for Signal {
    type Item = ();
    type Error = ();

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.rx_drained.poll() {
            Ok(Async::Ready(Some(..))) | Err(..) => {
                unreachable!("Receiver<Never> never receives a value")
            }
            Ok(Async::Ready(None)) => Ok(Async::Ready(())),
            Ok(Async::NotReady) => Ok(Async::NotReady),
        }
    }
}

#[derive(Clone)]
#[allow(missing_debug_implementations)]
pub(crate) struct Watch {
    rx: Shared<oneshot::Receiver<()>>,
    tx_drained: mpsc::Sender<Never>,
}

impl Watch {
    fn watch(&mut self) -> bool {
        match self.rx.poll() {
            Ok(Async::Ready(..)) | Err(..) => true,
            Ok(Async::NotReady) => false,
        }
    }

    pub(crate) fn watching<Fut, F>(self, future: Fut, on_drain: F) -> Watching<Fut, F>
    where
        Fut: Future,
        F: FnOnce(&mut Fut),
    {
        Watching {
            future,
            on_drain: Some(on_drain),
            watch: self,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Draining {
    rx_drained: mpsc::Receiver<Never>,
}

impl Future for Draining {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.rx_drained.poll() {
            Ok(Async::Ready(Some(..))) | Err(..) => {
                unreachable!("Receiver<Never> never receives a value")
            }
            Ok(Async::Ready(None)) => Ok(Async::Ready(())),
            Ok(Async::NotReady) => Ok(Async::NotReady),
        }
    }
}

#[allow(missing_debug_implementations)]
pub(crate) struct Watching<Fut, F> {
    future: Fut,
    on_drain: Option<F>,
    watch: Watch,
}

impl<Fut, F> Future for Watching<Fut, F>
where
    Fut: Future,
    F: FnOnce(&mut Fut),
{
    type Item = Fut::Item;
    type Error = Fut::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match self.on_drain.take() {
                Some(on_drain) => {
                    if self.watch.watch() {
                        on_drain(&mut self.future);
                        continue;
                    } else {
                        self.on_drain = Some(on_drain);
                        return self.future.poll();
                    }
                }
                None => return self.future.poll(),
            }
        }
    }
}

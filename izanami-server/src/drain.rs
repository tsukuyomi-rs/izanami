use {
    futures::{future::Shared, Async, Future, Poll, Stream},
    tokio::sync::{mpsc, oneshot},
};

// FIXME: replace with never type.
enum Never {}

pub(crate) fn channel() -> (Signal, Watch) {
    let (tx, rx) = oneshot::channel();
    let (tx_drained, rx_drained) = mpsc::channel(1);
    let signal = Signal {
        tx,
        draining: Draining { rx_drained },
    };
    let watch = Watch {
        rx: rx.shared(),
        tx_drained,
    };
    (signal, watch)
}

#[derive(Debug)]
pub(crate) struct Signal {
    tx: oneshot::Sender<()>,
    draining: Draining,
}

impl Signal {
    pub(crate) fn drain(self) -> Draining {
        self.draining
    }
}

impl Future for Signal {
    type Item = ();
    type Error = ();

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.draining.poll()
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

#[derive(Clone, Debug)]
pub(crate) struct Watch {
    rx: Shared<oneshot::Receiver<()>>,
    tx_drained: mpsc::Sender<Never>,
}

impl Watch {
    pub(crate) fn poll_signaled(&mut self) -> bool {
        match self.rx.poll() {
            Ok(Async::Ready(..)) | Err(..) => true,
            Ok(Async::NotReady) => false,
        }
    }
}

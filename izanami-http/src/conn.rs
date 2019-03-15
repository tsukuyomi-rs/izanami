use futures::{Future, Poll};

/// An asynchronous object that manages the connection to a remote peer.
pub trait Connection {
    /// The error type which will returned from this connection.
    type Error;

    /// Polls until the all incoming requests are handled and the connection is closed.
    fn poll_close(&mut self) -> Poll<(), Self::Error>;

    /// Notifies a shutdown signal to the connection.
    ///
    /// The connection may accept some requests for a while even after
    /// receiving this notification.
    fn graceful_shutdown(&mut self);
}

impl<F> Connection for F
where
    F: Future<Item = ()>,
{
    type Error = F::Error;

    fn poll_close(&mut self) -> Poll<(), Self::Error> {
        self.poll()
    }

    fn graceful_shutdown(&mut self) {
        // do nothing.
    }
}

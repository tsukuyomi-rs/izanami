use {
    futures::{Async, Future, Poll},
    std::{
        io,
        time::{Duration, Instant},
    },
    tokio_timer::Delay,
};

pub(super) trait Listener {
    type Conn;
    fn poll_accept(&mut self) -> Poll<Self::Conn, io::Error>;
}

#[derive(Debug)]
pub(super) struct SleepOnErrors<T> {
    listener: T,
    duration: Option<Duration>,
    timeout: Option<Delay>,
}

impl<T> SleepOnErrors<T>
where
    T: Listener,
{
    pub(super) fn new(listener: T) -> Self {
        Self {
            listener,
            duration: Some(Duration::from_secs(1)),
            timeout: None,
        }
    }

    pub(super) fn set_sleep_on_errors(&mut self, duration: Option<Duration>) {
        self.duration = duration;
    }

    #[inline]
    pub(super) fn poll_accept(&mut self) -> Poll<T::Conn, io::Error> {
        if let Some(timeout) = &mut self.timeout {
            match timeout.poll() {
                Ok(Async::Ready(())) => {}
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                Err(timer_err) => log::error!("sleep timer error: {}", timer_err),
            }
            self.timeout = None;
        }

        loop {
            match self.listener.poll_accept() {
                Ok(ok) => return Ok(ok),
                Err(ref err) if is_connection_error(err) => {
                    log::trace!("connection error: {}", err);
                    continue;
                }
                Err(err) => {
                    log::error!("accept error: {}", err);
                    if let Some(duration) = self.duration {
                        let mut timeout = Delay::new(Instant::now() + duration);
                        match timeout.poll() {
                            Ok(Async::Ready(())) => continue,
                            Ok(Async::NotReady) => {
                                log::error!("sleep until {:?}", timeout.deadline());
                                self.timeout = Some(timeout);
                                return Ok(Async::NotReady);
                            }
                            Err(timer_err) => {
                                log::error!("could not sleep: {}", timer_err);
                            }
                        }
                    }
                    return Err(err);
                }
            }
        }
    }
}

/// Returns whether the kind of provided error is caused by connection to the peer.
fn is_connection_error(err: &io::Error) -> bool {
    match err.kind() {
        io::ErrorKind::ConnectionRefused
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::ConnectionReset => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use {super::*, tokio::util::FutureExt};

    type DummyConnection = io::Cursor<Vec<u8>>;

    struct DummyListener {
        inner: std::collections::VecDeque<io::Result<DummyConnection>>,
    }

    impl Listener for DummyListener {
        type Conn = DummyConnection;

        fn poll_accept(&mut self) -> Poll<Self::Conn, io::Error> {
            let conn = self.inner.pop_front().expect("queue is empty")?;
            Ok(conn.into())
        }
    }

    #[test]
    fn ignore_connection_errors() -> io::Result<()> {
        let listener = DummyListener {
            inner: vec![
                Err(io::ErrorKind::ConnectionAborted.into()),
                Err(io::ErrorKind::ConnectionRefused.into()),
                Err(io::ErrorKind::ConnectionReset.into()),
                Ok(io::Cursor::new(vec![])),
            ]
            .into_iter()
            .collect(),
        };

        let mut listener = SleepOnErrors::new(listener);
        listener.set_sleep_on_errors(Some(Duration::from_micros(1)));

        let mut rt = tokio::runtime::current_thread::Runtime::new()?;

        let result = rt.block_on(
            futures::future::poll_fn({
                let listener = &mut listener;
                move || listener.poll_accept()
            })
            .timeout(Duration::from_millis(1)),
        );
        assert!(result.is_ok());

        Ok(())
    }

    #[test]
    fn sleep_on_errors() -> io::Result<()> {
        let listener = DummyListener {
            inner: vec![
                Err(io::Error::new(io::ErrorKind::Other, "Too many open files")),
                Ok(io::Cursor::new(vec![])),
            ]
            .into_iter()
            .collect(),
        };

        let mut listener = SleepOnErrors::new(listener);
        listener.set_sleep_on_errors(Some(Duration::from_micros(1)));

        let mut rt = tokio::runtime::current_thread::Runtime::new()?;

        let result = rt.block_on(
            futures::future::poll_fn({
                let listener = &mut listener;
                move || listener.poll_accept()
            })
            .timeout(Duration::from_millis(1)),
        );
        assert!(result.is_ok());

        Ok(())
    }

    #[test]
    fn abort_on_errors() -> io::Result<()> {
        let listener = DummyListener {
            inner: vec![
                Err(io::Error::new(io::ErrorKind::Other, "Too many open files")),
                Ok(io::Cursor::new(vec![])),
            ]
            .into_iter()
            .collect(),
        };

        let mut listener = SleepOnErrors::new(listener);
        listener.set_sleep_on_errors(None);

        let mut rt = tokio::runtime::current_thread::Runtime::new()?;

        let result = rt.block_on(
            futures::future::poll_fn({
                let listener = &mut listener;
                move || listener.poll_accept()
            })
            .timeout(Duration::from_millis(1)),
        );
        assert!(result.err().expect("should be failed").is_inner());

        Ok(())
    }
}

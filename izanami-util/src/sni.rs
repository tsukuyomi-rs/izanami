/// A type representing SNI hostname.
#[derive(Debug, Clone)]
pub struct SniHostname {
    inner: Option<String>,
}

impl SniHostname {
    /// Create a new `SniHostname` using the specified value.
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            inner: Some(value.into()),
        }
    }

    /// Create a new `SniHostname` indicating that the SNI hostname is unavailable.
    pub fn unknown() -> Self {
        Self { inner: None }
    }

    /// Returns a reference to the inner value if available.
    pub fn as_ref(&self) -> Option<&str> {
        self.inner.as_ref().map(|s| &**s)
    }
}

impl<S> From<Option<S>> for SniHostname
where
    S: Into<String>,
{
    fn from(value: Option<S>) -> Self {
        Self {
            inner: value.map(Into::into),
        }
    }
}

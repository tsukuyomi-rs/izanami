use std::fmt;

pub(crate) type BoxedStdError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug)]
pub struct Error {
    compat: Compat,
}

impl Error {
    pub(crate) fn from_boxed_compat(err: impl Into<BoxedStdError>) -> Self {
        failure::Error::from_boxed_compat(err.into()) //
            .into()
    }

    pub fn compat(self) -> Compat {
        self.compat
    }
}

impl fmt::Display for Error {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.compat.fmt(f)
    }
}

#[derive(Debug, failure::Fail)]
pub enum Compat {
    #[fail(display = "custom error: {}", _0)]
    Custom(failure::Error),
}

impl<E> From<E> for Error
where
    E: Into<failure::Error>,
{
    fn from(err: E) -> Self {
        Self {
            compat: Compat::Custom(err.into()),
        }
    }
}

pub type Result<T = ()> = std::result::Result<T, Error>;

use {
    futures::Future,
    http::Request,
    tokio_io::{AsyncRead, AsyncWrite},
};

pub trait Upgradable {
    type Upgraded: AsyncRead + AsyncWrite;
    type Error;
    type OnUpgrade: Future<Item = Self::Upgraded, Error = Self::Error>;

    fn on_upgrade(self) -> Self::OnUpgrade;
}

impl<T> Upgradable for Request<T>
where
    T: Upgradable,
{
    type Upgraded = T::Upgraded;
    type Error = T::Error;
    type OnUpgrade = T::OnUpgrade;

    fn on_upgrade(self) -> Self::OnUpgrade {
        self.into_body().on_upgrade()
    }
}

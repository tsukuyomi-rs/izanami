/// A type representing the server name provided by SNI.
#[derive(Debug, Clone)]
pub enum ServerName {
    /// DNS hostname.
    Dns(String),

    /// Opaque name type.
    #[allow(missing_docs)]
    Opaque {
        name_type: Vec<u8>,
        host_name: Vec<u8>,
    },

    #[doc(hidden)]
    __NonExhausive(()),
}

impl ServerName {
    /// Returns whether the type of hostname is DNS.
    pub fn is_dns(&self) -> bool {
        match self {
            ServerName::Dns(..) => true,
            _ => false,
        }
    }
}

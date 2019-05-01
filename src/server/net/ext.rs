//! Types representing the information extracted from the transport.
//!
//! These values are typically inserted into the request's extension map
//! by the server.

mod remote;
mod sni;

pub use {
    self::remote::RemoteAddr, //
    self::sni::ServerName,
};

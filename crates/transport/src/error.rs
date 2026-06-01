//! Transport errors.

use thiserror::Error;

/// Something went wrong moving notifications between a producer and a consumer.
#[derive(Debug, Error)]
pub enum TransportError {
    /// Could not establish or hold the connection.
    #[error("connection failed: {0}")]
    Connect(String),
    /// The server answered, but not with success.
    #[error("HTTP error: {0}")]
    Http(String),
    /// A frame arrived but could not be decoded into a notification.
    #[error("decode error: {0}")]
    Decode(String),
}

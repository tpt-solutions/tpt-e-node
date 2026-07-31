use core::fmt;

use tpt_e_link::LinkError;

/// Errors that can originate from `tpt-node-core` itself (as opposed to the
/// underlying `LinkTransport` or framing layer, which surface as
/// [`LinkError`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NodeError {
    /// The underlying transport or framing layer returned an error.
    Transport(LinkError),
    /// The capability registry is full (already at `MAX_CAPABILITIES`).
    RegistryFull,
    /// A postcard encode/decode failure occurred internally.
    CodecError,
    /// An OTA update failed (bad CRC, timeout, etc.).
    OtaFailed,
    /// A driver's underlying I/O failed (host-backed mock drivers, etc.).
    Io,
    /// The reactor was asked to do something that doesn't make sense in its
    /// current state.
    InvalidState,
}

impl fmt::Display for NodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeError::Transport(e) => write!(f, "transport error: {e}"),
            NodeError::RegistryFull => write!(f, "capability registry is full"),
            NodeError::CodecError => write!(f, "postcard encode/decode error"),
            NodeError::OtaFailed => write!(f, "OTA update failed"),
            NodeError::Io => write!(f, "driver I/O error"),
            NodeError::InvalidState => write!(f, "invalid reactor state"),
        }
    }
}

impl From<LinkError> for NodeError {
    fn from(e: LinkError) -> Self {
        NodeError::Transport(e)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for NodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            NodeError::Transport(e) => Some(e),
            _ => None,
        }
    }
}

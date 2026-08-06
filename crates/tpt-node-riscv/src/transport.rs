//! Link transports implementing [`tpt_e_link::LinkTransport`] for generic
//! RISC-V chips.
//!
//! Compiled only when a chip feature (`gd32v`, `sifive`) is enabled, since the
//! concrete BSP `Serial` types are feature-gated downstream. The transport
//! itself is generic over any `embedded-hal`/`embedded-hal-nb` serial half, so
//! the framing logic is shared across every supported part.

// Available for real hardware (gd32v/sifive) *and* on the host (std) so the
// transport's framing logic can be unit-tested without pulling in a BSP.
#[cfg(any(feature = "gd32v", feature = "sifive", feature = "std"))]
pub mod uart;

#[cfg(any(feature = "gd32v", feature = "sifive"))]
pub use uart::{BlockingUartTransport, UartTransport};

//! `tpt-node-esp32` — ESP32 HAL for TPT Embedded Node.
//!
//! Provides real-hardware [`tpt_e_link::LinkTransport`] implementations and
//! flash-backed drivers for the `tpt-node-core` reactor on the ESP32 family:
//!
//! - [`transport::uart::UartTransport`] — an async UART transport driven by
//!   `esp-hal`'s interrupt-backed `Uart<'_, Async, _>` (compiled for any
//!   enabled chip feature).
//! - [`ble::BleTransport`] — a transport that carries `tpt-e-link` frames over
//!   a GATT byte pipe (the protocol layer; the concrete NimBLE/esp-wifi
//!   adapter is documented in [`ble`]).
//! - [`flash::OtaRegionFlashWriter`] — wires the internal OTA handler from
//!   `tpt-node-core` to real ESP32 flash via `embedded-storage`, with an
//!   incremental CRC32 so images are verified without buffering them in RAM.
//! - [`flash::RegionStorage`] — a sub-region view over a `NorFlash` device,
//!   used to carve a LittleFS partition out of the SPI flash.
//! - [`littlefs::EspLittleFsDriver`] (feature `fs`) — a real
//!   [`tpt_node_core::LittleFsDriver`] backed by `littlefs2`.
//!
//! The [`tpt_e_typestate_hal`] safe DMA/ISR layer is wired in as a dependency
//! so device firmware can use typestate-checked DMA for ring-buffer handoff
//! and hardware crypto without re-implementing register access.
//!
//! # Chip selection
//!
//! At most one of the `esp32`, `esp32s3`, `esp32c3`, `esp32c6` features may
//! be enabled. None are on by default so `cargo build --workspace` on a host
//! target keeps working. Build for real hardware with e.g.:
//!
//! ```text
//! cargo build -p tpt-node-esp32 --release --target riscv32imc-unknown-none-elf --features esp32c3
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod ble;
pub mod crc;
pub mod flash;
#[cfg(feature = "fs")]
pub mod littlefs;
pub mod transport;

#[cfg(test)]
pub(crate) mod test_util;

pub use ble::{BleStream, BleTransport};
pub use crc::IsoHdlcCrc32;
pub use flash::{OtaRegionFlashWriter, RegionStorage};
#[cfg(feature = "fs")]
pub use littlefs::EspLittleFsDriver;

/// Maximum LittleFS partition size this crate's bindings are sized for
/// (256 KiB). Enough for config + modest data on the default ESP32-C3
/// 4 MiB part.
pub const LITTLEFS_REGION_SIZE: u32 = 256 * 1024;

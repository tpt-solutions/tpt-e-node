//! Link transports implementing [`tpt_e_link::LinkTransport`] for the ESP32
//! family.
//!
//! Compiled only when a chip feature (`esp32`, `esp32s3`, `esp32c3`,
//! `esp32c6`) is enabled, since `esp-hal` itself can only be built for the
//! embedded targets.

#[cfg(any(feature = "esp32", feature = "esp32s3", feature = "esp32c3", feature = "esp32c6"))]
pub mod uart;

#[cfg(any(feature = "esp32", feature = "esp32s3", feature = "esp32c3", feature = "esp32c6"))]
pub use uart::{BlockingUartTransport, UartTransport};

// Classic ESP32 (Xtensa) has no USB-Serial-JTAG peripheral; only the
// S3/C3/C6 parts do.
#[cfg(any(feature = "esp32s3", feature = "esp32c3", feature = "esp32c6"))]
pub mod usb_serial_jtag;

#[cfg(any(feature = "esp32s3", feature = "esp32c3", feature = "esp32c6"))]
pub use usb_serial_jtag::UsbSerialJtagTransport;

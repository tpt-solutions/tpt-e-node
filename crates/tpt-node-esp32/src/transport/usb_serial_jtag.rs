//! `tpt-e-link` transport over the native USB-Serial-JTAG peripheral.
//!
//! `esp32s3`/`esp32c3`/`esp32c6` expose a built-in USB CDC-ACM endpoint (no
//! external USB-UART bridge chip required); boards that only break out this
//! port — like most ESP32-C3-DevKitM/DevKitC boards — cannot use
//! [`super::uart::UartTransport`] at all since there is no wired UART. This
//! is the blocking counterpart to [`super::uart::BlockingUartTransport`]: it
//! busy-waits on [`UsbSerialJtag`]'s register-level FIFO, so — like
//! `BlockingUartTransport` — every future it produces completes on first
//! poll and the `tpt-node-core` reactor can be driven by a trivial busy-loop
//! `block_on`, no executor required. Ideal for first bring-up; once other
//! tasks need CPU time, move to an interrupt/async-driven transport instead.

use esp_hal::prelude::nb;
use esp_hal::usb_serial_jtag::UsbSerialJtag;
use esp_hal::Blocking;
use tpt_e_link::{LinkError, LinkTransport};

/// A `tpt-e-link` transport over the blocking-mode USB-Serial-JTAG
/// peripheral.
pub struct UsbSerialJtagTransport {
    usb: UsbSerialJtag<'static, Blocking>,
}

impl UsbSerialJtagTransport {
    /// Wraps a blocking-mode USB-Serial-JTAG peripheral.
    pub fn new(usb: UsbSerialJtag<'static, Blocking>) -> Self {
        Self { usb }
    }
}

impl LinkTransport for UsbSerialJtagTransport {
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), LinkError> {
        for byte in buf.iter_mut() {
            *byte = nb::block!(self.usb.read_byte()).map_err(|_| LinkError::Io)?;
        }
        Ok(())
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<(), LinkError> {
        self.usb.write_bytes(buf).map_err(|_| LinkError::Io)?;
        self.usb.flush_tx().map_err(|_| LinkError::Io)
    }
}

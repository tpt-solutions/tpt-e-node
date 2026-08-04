//! Async UART transport for `tpt-e-link` framing over a hardware UART.
//!
//! Built on `esp-hal`'s interrupt-driven async UART: the driver claims the
//! whole [`Uart`](esp_hal::uart::Uart) peripheral in [`Async`](esp_hal::Async)
//! mode so the `Node` reactor from `tpt-node-core` can run on a cooperative
//! executor (e.g. `embassy-executor`) while UART interrupts keep the RX FIFO
//! drained.
//!
//! # Wiring into `tpt-node-core`
//!
//! ```text
//! let uart = esp_hal::uart::Uart::new_with_config(
//!     peripherals.UART0,
//!     config,
//!     peripherals.GPIO10,  // RX
//!     peripherals.GPIO9,   // TX
//! )?
//! .into_async();
//! let transport = UartTransport::new(uart);
//! let mut node = tpt_node_core::Node::new(transport, *b"mydevice", Version::new(1), Arch::RiscV32Imc);
//! node.register_capability(Capability::Ota);
//! node.run().await;
//! ```
//!
//! `read_exact` maps every esp-hal error to [`LinkError::Io`] (the wire layer
//! retries framing), and treats a zero-byte read as [`LinkError::UnexpectedEof`]
//! to avoid spinning (esp-hal's `read_async` never returns `Ok(0)`, so this
//! only triggers in defensive edge cases).

use esp_hal::uart::{Instance, Uart};
use esp_hal::Async;
use tpt_e_link::{LinkError, LinkTransport};

/// A `tpt-e-link` transport that sends and receives framed messages over a
/// single async hardware UART.
///
/// `T` is the UART instance type (`Uart0`, `Uart1`, ... or `AnyUart`).
/// Generic over the instance so a single implementation covers every ESP32
/// part without recompiling the framing logic.
pub struct UartTransport<T> {
    uart: Uart<'static, Async, T>,
}

impl<T> UartTransport<T>
where
    T: Instance,
{
    /// Wraps an async-mode UART peripheral.
    pub fn new(uart: Uart<'static, Async, T>) -> Self {
        Self { uart }
    }
}

impl<T> LinkTransport for UartTransport<T>
where
    T: Instance,
{
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), LinkError> {
        let mut filled = 0;
        while filled < buf.len() {
            let n = self
                .uart
                .read_async(&mut buf[filled..])
                .await
                .map_err(|_| LinkError::Io)?;
            if n == 0 {
                return Err(LinkError::UnexpectedEof);
            }
            filled += n;
        }
        Ok(())
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<(), LinkError> {
        let mut written = 0;
        while written < buf.len() {
            written += self
                .uart
                .write_async(&buf[written..])
                .await
                .map_err(|_| LinkError::Io)?;
        }
        self.uart.flush_async().await.map_err(|_| LinkError::Io)
    }
}

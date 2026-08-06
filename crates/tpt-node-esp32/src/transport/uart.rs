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

use esp_hal::prelude::nb;
use esp_hal::uart::{Instance, Uart};
use esp_hal::{Async, Blocking};
use tpt_e_link::{LinkError, LinkTransport};

/// A `tpt-e-link` transport that sends and receives framed messages over a
/// single async hardware UART.
///
/// `T` is the UART instance type (`Uart0`, `Uart1`, ... or `AnyUart`).
/// Generic over the instance so a single implementation covers every ESP32
/// part without recompiling the framing logic.
pub struct UartTransport<T: 'static> {
    uart: Uart<'static, Async, T>,
}

impl<T> UartTransport<T>
where
    T: Instance + 'static,
{
    /// Wraps an async-mode UART peripheral.
    pub fn new(uart: Uart<'static, Async, T>) -> Self {
        Self { uart }
    }
}

impl<T> LinkTransport for UartTransport<T>
where
    T: Instance + 'static,
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

/// A `tpt-e-link` transport over a *blocking* hardware UART.
///
/// This is the executor-free counterpart to [`UartTransport`]: the UART stays
/// in [`Blocking`](esp_hal::Blocking) mode and every [`read_exact`] /
/// [`write_all`] call blocks until the byte buffer is satisfied. Because the
/// underlying operations never return "would block", a future produced by
/// these methods completes on its first poll — so the `tpt-node-core` reactor
/// can be driven with a trivial busy-loop `block_on` (no `embassy-executor`,
/// no interrupts, no timer tick) at the cost of burning CPU while waiting for
/// the link. This is ideal for a first bring-up / handshake milestone; switch
/// to the interrupt-driven [`UartTransport`] + `embassy-executor` when other
/// tasks need CPU time.
///
/// [`read_exact`]: tpt_e_link::LinkTransport::read_exact
/// [`write_all`]: tpt_e_link::LinkTransport::write_all
pub struct BlockingUartTransport<T: 'static> {
    uart: Uart<'static, Blocking, T>,
}

impl<T> BlockingUartTransport<T>
where
    T: Instance + 'static,
{
    /// Wraps a blocking-mode UART peripheral.
    pub fn new(uart: Uart<'static, Blocking, T>) -> Self {
        Self { uart }
    }
}

impl<T> LinkTransport for BlockingUartTransport<T>
where
    T: Instance + 'static,
{
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), LinkError> {
        self.uart.read_bytes(buf).map_err(|_| LinkError::Io)
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<(), LinkError> {
        self.uart
            .write_bytes(buf)
            .map_err(|_| LinkError::Io)?;
        loop {
            match self.uart.flush_tx() {
                Ok(()) => return Ok(()),
                Err(nb::Error::WouldBlock) => {}
                Err(_) => return Err(LinkError::Io),
            }
        }
    }
}

//! UART transport for `tpt-e-link` framing over `embedded-hal` serial halves.
//!
//! Built on the `embedded-hal` `Write<u8>` / `embedded-hal-nb` `Read<u8>`
//! traits, which every generic RISC-V BSP (GD32VF103, SiFive E310, ...) and
//! indeed any `embedded-hal` serial device implements. A single generic
//! implementation therefore covers every chip this crate targets.
//!
//! Two flavours are provided:
//!
//! - [`UartTransport`] — an *async* transport. Its futures yield (return
//!   `Poll::Pending`) when the half returns [`nb::Error::WouldBlock`], so a
//!   real interrupt-driven executor (e.g. `embassy-executor`) keeps the CPU
//!   free while the RX FIFO fills. A peripheral interrupt must wake the task
//!   for progress to resume.
//! - [`BlockingUartTransport`] — an *executor-free* transport. Every read and
//!   write busy-loops until satisfied, so the `tpt-node-core` reactor can be
//!   driven with a trivial busy-loop `block_on` for first bring-up (no
//!   executor, no interrupts). This mirrors `tpt-node-esp32`'s
//!   `BlockingUartTransport`.
//!
//! `read_exact` maps every `nb` error to [`LinkError::Io`] (the wire layer
//! retries framing).

use core::future;
use core::task::Poll;

use embedded_hal_nb::serial::{Read as SerialRead, Write as SerialWrite};
use tpt_e_link::{LinkError, LinkTransport};

/// An async `tpt-e-link` transport sending/receiving framed messages over two
/// `embedded-hal` serial halves (`TX` = `Write<u8>`, `RX` = `Read<u8>`).
///
/// Generic over the halves so the same framing logic serves every supported
/// RISC-V BSP without recompiling.
pub struct UartTransport<TX, RX> {
    tx: TX,
    rx: RX,
}

impl<TX, RX> UartTransport<TX, RX> {
    /// Wraps a transmit half and a receive half.
    pub fn new(tx: TX, rx: RX) -> Self {
        Self { tx, rx }
    }
}

impl<TX, RX> LinkTransport for UartTransport<TX, RX>
where
    TX: SerialWrite<u8>,
    RX: SerialRead<u8>,
{
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), LinkError> {
        let mut filled = 0;
        while filled < buf.len() {
            future::poll_fn(|_| match self.rx.read() {
                Ok(b) => {
                    buf[filled] = b;
                    Poll::Ready(Ok(()))
                }
                Err(nb::Error::WouldBlock) => Poll::Pending,
                Err(nb::Error::Other(_)) => Poll::Ready(Err(LinkError::Io)),
            })
            .await?;
            filled += 1;
        }
        Ok(())
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<(), LinkError> {
        for &b in buf {
            future::poll_fn(|_| match self.tx.write(b) {
                Ok(()) => Poll::Ready(Ok(())),
                Err(nb::Error::WouldBlock) => Poll::Pending,
                Err(nb::Error::Other(_)) => Poll::Ready(Err(LinkError::Io)),
            })
            .await?;
        }
        future::poll_fn(|_| match self.tx.flush() {
            Ok(()) => Poll::Ready(Ok(())),
            Err(nb::Error::WouldBlock) => Poll::Pending,
            Err(nb::Error::Other(_)) => Poll::Ready(Err(LinkError::Io)),
        })
        .await
    }
}

/// A `tpt-e-link` transport over *blocking* `embedded-hal` serial halves.
///
/// Each read byte and written word busy-loops until satisfied (the
/// `Write`/`Read` traits never permanently return "would block" on a blocking
/// peripheral — `WouldBlock` simply means "not yet", so spinning makes
/// forward progress). The futures therefore complete on their first poll once
/// the link catches up, letting the `tpt-node-core` reactor be driven by a
/// trivial busy-loop `block_on` with no executor or interrupts. Switch to
/// [`UartTransport`] + `embassy-executor` when other tasks need CPU time.
pub struct BlockingUartTransport<TX, RX> {
    tx: TX,
    rx: RX,
}

impl<TX, RX> BlockingUartTransport<TX, RX> {
    /// Wraps a transmit half and a receive half.
    pub fn new(tx: TX, rx: RX) -> Self {
        Self { tx, rx }
    }
}

impl<TX, RX> LinkTransport for BlockingUartTransport<TX, RX>
where
    TX: SerialWrite<u8>,
    RX: SerialRead<u8>,
{
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), LinkError> {
        for slot in buf.iter_mut() {
            loop {
                match self.rx.read() {
                    Ok(b) => {
                        *slot = b;
                        break;
                    }
                    Err(nb::Error::WouldBlock) => continue,
                    Err(nb::Error::Other(_)) => return Err(LinkError::Io),
                }
            }
        }
        Ok(())
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<(), LinkError> {
        for &b in buf {
            loop {
                match self.tx.write(b) {
                    Ok(()) => break,
                    Err(nb::Error::WouldBlock) => continue,
                    Err(nb::Error::Other(_)) => return Err(LinkError::Io),
                }
            }
        }
        loop {
            match self.tx.flush() {
                Ok(()) => return Ok(()),
                Err(nb::Error::WouldBlock) => continue,
                Err(nb::Error::Other(_)) => return Err(LinkError::Io),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::rc::Rc;
    use alloc::vec::Vec;
    use core::cell::RefCell;

    /// Shared state for an in-memory link. A real UART has separate TX and RX
    /// lines, so `tx_buf` holds bytes the device writes and `rx_buf` holds
    /// bytes the peer transmits. Used to exercise the transport on the host
    /// without any real hardware.
    struct Inner {
        tx_buf: Vec<u8>,
        rx_buf: Vec<u8>,
    }

    impl Inner {
        fn new() -> Rc<RefCell<Self>> {
            Rc::new(RefCell::new(Self { tx_buf: Vec::new(), rx_buf: Vec::new() }))
        }
    }

    /// Write endpoint: outbound bytes accumulate in `tx_buf`.
    #[derive(Clone)]
    struct Tx {
        inner: Rc<RefCell<Inner>>,
    }

    /// Read endpoint: drains `rx_buf`.
    #[derive(Clone)]
    struct Rx {
        inner: Rc<RefCell<Inner>>,
    }

    impl Tx {
        fn new(inner: Rc<RefCell<Inner>>) -> Self {
            Self { inner }
        }
    }

    impl Rx {
        fn new(inner: Rc<RefCell<Inner>>) -> Self {
            Self { inner }
        }
        /// Queues bytes to be read (simulates the peer transmitting).
        fn feed(&self, bytes: &[u8]) {
            self.inner.borrow_mut().rx_buf.extend_from_slice(bytes);
        }
    }

    impl embedded_hal_nb::serial::ErrorType for Tx {
        type Error = embedded_hal_nb::serial::ErrorKind;
    }
    impl embedded_hal_nb::serial::ErrorType for Rx {
        type Error = embedded_hal_nb::serial::ErrorKind;
    }
    impl SerialWrite<u8> for Tx {
        fn write(&mut self, word: u8) -> nb::Result<(), Self::Error> {
            self.inner.borrow_mut().tx_buf.push(word);
            Ok(())
        }
        fn flush(&mut self) -> nb::Result<(), Self::Error> {
            Ok(())
        }
    }
    impl SerialRead<u8> for Rx {
        fn read(&mut self) -> nb::Result<u8, Self::Error> {
            let mut inner = self.inner.borrow_mut();
            match inner.rx_buf.first() {
                Some(&b) => {
                    inner.rx_buf.remove(0);
                    Ok(b)
                }
                None => Err(nb::Error::WouldBlock),
            }
        }
    }

    fn link() -> (Tx, Rx) {
        let inner = Inner::new();
        (Tx::new(inner.clone()), Rx::new(inner))
    }

    #[test]
    fn blocking_roundtrip() {
        let (tx, rx) = link();
        let captured = tx.inner.clone();
        let mut transport = BlockingUartTransport::new(tx, rx.clone());

        pollster::block_on(async {
            transport.write_all(b"hello tpt").await.unwrap();
            assert_eq!(&captured.borrow().tx_buf, b"hello tpt");

            let mut out = [0u8; 4];
            rx.feed(b"pong");
            transport.read_exact(&mut out).await.unwrap();
            assert_eq!(&out, b"pong");
            Ok::<(), LinkError>(())
        })
        .unwrap();
    }

    #[test]
    fn async_roundtrip() {
        let (tx, rx) = link();
        let mut transport = UartTransport::new(tx, rx.clone());

        pollster::block_on(async {
            transport.write_all(b"abc").await.unwrap();

            let mut out = [0u8; 3];
            rx.feed(b"xyz");
            transport.read_exact(&mut out).await.unwrap();
            assert_eq!(&out, b"xyz");
            Ok::<(), LinkError>(())
        })
        .unwrap();
    }

    #[test]
    fn async_read_blocks_until_data() {
        let (tx, rx) = link();
        let mut transport = UartTransport::new(tx, rx.clone());

        let result = pollster::block_on(async {
            // Nothing queued yet: the read future must yield then complete once
            // data arrives (the busy-loop executor re-polls after we feed).
            let mut out = [0u8; 2];
            rx.feed(b"QR");
            transport.read_exact(&mut out).await?;
            Ok::<[u8; 2], LinkError>(out)
        });
        assert_eq!(result.unwrap(), *b"QR");
    }
}

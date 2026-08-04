//! BLE link transport.
//!
//! The BLE radio is deliberately left out of this crate for now: the
//! `esp-wifi` release whose GATT API matches the pinned esp-hal 0.22 stack is
//! still being audited (esp-wifi's NimBLE bindings changed substantially
//! across its 0.15.x line), so pulling it in would pin the whole HAL stack to
//! a possibly wrong version. Instead this module defines a thin
//! [`BleStream`] byte-pipe trait and a [`BleTransport`] adapter that turns any
//! `BleStream` into a [`tpt_e_link::LinkTransport`] — a GATT service on top of
//! esp-wifi can implement `BleStream` on the application side.

use tpt_e_link::{error::LinkError, LinkTransport};

/// Size of the read-side buffer that stages bytes from the stream.
const RX_BUF_LEN: usize = 64;

/// A bidirectional byte pipe over a BLE GATT connection.
///
/// Implementations drive the GATT link (e.g. via esp-wifi's NimBLE stack) and
/// present a blocking byte stream: `read` returns as soon as *some* bytes are
/// available, `write` sends everything before returning.
pub trait BleStream {
    /// The stream's native error type; it is mapped to
    /// [`LinkError::Io`] by [`BleTransport`].
    type Error;

    /// Blocks until at least one byte is available, then returns the number of
    /// bytes copied into `buf` (`1 ..= buf.len()`).
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error>;

    /// Sends every byte in `buf`, blocking until the transport has accepted
    /// all of them.
    fn write(&mut self, buf: &[u8]) -> Result<(), Self::Error>;
}

/// A [`tpt_e_link::LinkTransport`] backed by a [`BleStream`].
pub struct BleTransport<S> {
    stream: S,
    rx_buf: [u8; RX_BUF_LEN],
    rx_pos: usize,
    rx_len: usize,
}

impl<S> BleTransport<S> {
    /// Creates a transport over `stream`.
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            rx_buf: [0u8; RX_BUF_LEN],
            rx_pos: 0,
            rx_len: 0,
        }
    }

    /// Consumes the transport and returns the underlying stream.
    pub fn into_inner(self) -> S {
        self.stream
    }
}

impl<S: BleStream> LinkTransport for BleTransport<S> {
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), LinkError> {
        let mut filled = 0;
        while filled < buf.len() {
            if self.rx_pos == self.rx_len {
                self.rx_len = self.stream.read(&mut self.rx_buf).map_err(|_| LinkError::Io)?;
                self.rx_pos = 0;
                if self.rx_len == 0 {
                    return Err(LinkError::UnexpectedEof);
                }
            }
            let n = (self.rx_len - self.rx_pos).min(buf.len() - filled);
            buf[filled..filled + n]
                .copy_from_slice(&self.rx_buf[self.rx_pos..self.rx_pos + n]);
            self.rx_pos += n;
            filled += n;
        }
        Ok(())
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<(), LinkError> {
        self.stream.write(buf).map_err(|_| LinkError::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;

    /// A scripted byte-pipe for tests: pushes bytes in small reads to exercise
    /// the staging buffer across `read_exact` calls.
    struct ScriptedStream {
        data: &'static [u8],
        pos: Cell<usize>,
        writes: Cell<usize>,
    }

    impl ScriptedStream {
        fn new(data: &'static [u8]) -> Self {
            Self {
                data,
                pos: Cell::new(0),
                writes: Cell::new(0),
            }
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct StreamError;

    impl BleStream for ScriptedStream {
        type Error = StreamError;

        fn read(&mut self, buf: &mut [u8]) -> Result<usize, StreamError> {
            let avail = self.data.len().saturating_sub(self.pos.get());
            if avail == 0 {
                return Ok(0);
            }
            let n = avail.min(buf.len()).min(5);
            buf[..n].copy_from_slice(&self.data[self.pos.get()..self.pos.get() + n]);
            self.pos.set(self.pos.get() + n);
            Ok(n)
        }

        fn write(&mut self, buf: &[u8]) -> Result<(), StreamError> {
            self.writes.set(self.writes.get() + buf.len());
            Ok(())
        }
    }

    fn test_transport(data: &'static [u8]) -> BleTransport<ScriptedStream> {
        BleTransport::new(ScriptedStream::new(data))
    }

    #[test]
    fn read_exact_spans_multiple_stream_reads() {
        let mut t = test_transport(b"abcdefghij");
        let mut buf = [0u8; 10];
        pollster::block_on(t.read_exact(&mut buf)).expect("read");
        assert_eq!(&buf, b"abcdefghij");
    }

    #[test]
    fn read_exact_incremental() {
        let mut t = test_transport(b"hello world");
        let mut a = [0u8; 5];
        pollster::block_on(t.read_exact(&mut a)).expect("read");
        assert_eq!(&a, b"hello");
        let mut b = [0u8; 6];
        pollster::block_on(t.read_exact(&mut b)).expect("read");
        assert_eq!(&b, b" world");
    }

    #[test]
    fn read_exact_hits_eof() {
        let mut t = test_transport(b"ab");
        let mut buf = [0u8; 4];
        assert!(pollster::block_on(t.read_exact(&mut buf)).is_err());
    }

    #[test]
    fn write_all_passes_bytes_through() {
        let mut t = test_transport(b"");
        pollster::block_on(t.write_all(b"xyz")).expect("write");
        assert_eq!(t.stream.writes.get(), 3);
    }
}

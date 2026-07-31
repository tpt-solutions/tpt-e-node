//! `tpt-node-mock` — mock-first TPT device.
//!
//! A `std` binary + library that implements [`tpt_e_link::LinkTransport`]
//! over a local WebSocket connection, then runs the real
//! [`tpt_node_core::Node`] reactor on top of it with fake, host-backed
//! capabilities:
//!
//! - **LittleFS** — a simulated filesystem backed by a real directory on the
//!   host's hard drive (`HostLittleFsDriver`).
//! - **OTA** — the internal `tpt-node-core` handler with an in-RAM flash
//!   writer.
//! - **Telemetry** — a simulated sensor driver.
//!
//! This lets an engineer develop and test device logic, OTA flows, and
//! capability-gated BaseStation UI entirely on a laptop, before compiling for
//! a physical microcontroller.
//!
//! ## Running
//!
//! ```sh
//! cargo run -p tpt-node-mock
//! ```
//!
//! The mock connects to `ws://127.0.0.1:8375/ws` by default, so BaseStation
//! should be running its WebSocket listener (`start_ws_listener`).
//!
//! | Environment variable | Default | Meaning |
//! |----------------------|---------|---------|
//! | `TPT_WS_URL`         | `ws://127.0.0.1:8375/ws` | WebSocket endpoint to connect to |
//! | `TPT_MOCK_FS`        | `mock-fs` | Directory backing the simulated LittleFS |
//! | `TPT_MOCK_DEVICE_ID` | `tptmock1` | 8-byte device ID |
//!
//! The mock reconnects forever (2s backoff), so BaseStation can restart
//! without restarting the mock.

use std::collections::VecDeque;
use std::net::TcpStream;

pub use drivers::{HostLittleFsDriver, MockTelemetryDriver};
use tpt_e_link::LinkError;
use tungstenite::protocol::WebSocket;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::Message;

pub mod drivers;

/// A [`tpt_e_link::LinkTransport`] backed by a (blocking) WebSocket client.
///
/// `read_exact` buffers incoming `Binary` messages and serves bytes out of
/// the buffer until `buf` is full; `write_all` sends the whole buffer as a
/// single `Binary` message. Like `tpt-e-link`'s `MockTcpTransport`, the
/// blocking calls resolve on first poll — perfectly fine for a single-
/// threaded mock.
pub struct WsTransport {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    pending: VecDeque<u8>,
}

impl WsTransport {
    /// Wraps an already-connected WebSocket.
    pub fn new(ws: WebSocket<MaybeTlsStream<TcpStream>>) -> Self {
        Self {
            ws,
            pending: VecDeque::new(),
        }
    }

    /// Connects to `url` and wraps the resulting socket.
    ///
    /// # Errors
    ///
    /// Returns `LinkError::Io` if the connection handshake fails.
    pub fn connect(url: &str) -> Result<Self, LinkError> {
        let (ws, _) = tungstenite::connect(url).map_err(|_| LinkError::Io)?;
        Ok(Self::new(ws))
    }

    /// Clones the underlying plain-TCP socket, so a caller can force the
    /// connection closed (e.g. `TcpStream::shutdown`) from outside the
    /// `LinkTransport`, without needing a `&mut` reference into whatever
    /// currently owns this transport (typically a `Node` blocked inside its
    /// reactor loop on another thread).
    ///
    /// Returns `None` if the connection is TLS-wrapped (`wss://`) — there is
    /// no plain socket to clone in that case.
    pub fn try_clone_stream(&self) -> std::io::Result<Option<TcpStream>> {
        match self.ws.get_ref() {
            MaybeTlsStream::Plain(stream) => stream.try_clone().map(Some),
            _ => Ok(None),
        }
    }
}

impl tpt_e_link::LinkTransport for WsTransport {
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), LinkError> {
        let mut filled = 0;
        while filled < buf.len() {
            if let Some(byte) = self.pending.pop_front() {
                buf[filled] = byte;
                filled += 1;
                continue;
            }
            match self.ws.read() {
                Ok(Message::Binary(data)) => self.pending.extend(data),
                Ok(Message::Text(text)) => self.pending.extend(text.as_bytes()),
                Ok(Message::Ping(payload)) => {
                    let _ = self.ws.send(Message::Pong(payload));
                }
                // Pong / Close / Frame — nothing to buffer.
                Ok(_) => {}
                Err(_) => return Err(LinkError::Io),
            }
        }
        Ok(())
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<(), LinkError> {
        self.ws.send(Message::Binary(buf.to_vec())).map_err(|_| LinkError::Io)
    }
}

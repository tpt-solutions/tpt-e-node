//! Internal OTA (Over-The-Air) update handler.
//!
//! When [`crate::Capability::Ota`] is registered, `tpt-node-core` automatically
//! handles chunked firmware download, CRC32 verification, and flash writing —
//! the user does not need to write any OTA boilerplate.
//!
//! ## Protocol
//!
//! The OTA flow uses three command IDs in the `0x02xx` range:
//!
//! | Command     | ID      | Args (LE)                                         |
//! |-------------|---------|---------------------------------------------------|
//! | `OtaStart`  | `0x0200`| `expected_size: u32` + `expected_crc: u32` (8 B)  |
//! | `OtaChunk`  | `0x0201`| `offset: u32` + `data: [u8; ≤28]`                 |
//! | `OtaFinish` | `0x0202`| *(empty)*                                         |
//!
//! The host sends `OtaStart` to announce the image size and CRC, then streams
//! `OtaChunk` messages (each ≤ 28 bytes of payload due to `MAX_COMMAND_ARGS_LEN`
//! = 32 and the 4-byte offset header), and finally `OtaFinish` to signal
//! completion. The handler writes each chunk to the [`FlashWriter`] and
//! verifies the CRC on `OtaFinish`.

use alloc::boxed::Box;
use core::fmt;

use tpt_e_link::message::{AckPayload, CommandPayload};

use crate::capability::cmd;
use crate::error::NodeError;

/// Maximum OTA image size buffered in RAM when no external [`FlashWriter`]
/// is provided (used by the mock/test path).
pub const MAX_OTA_BUFFER_SIZE: usize = 1024;

/// Trait for writing OTA firmware images to flash.
///
/// HAL crates provide a concrete implementation backed by real flash hardware;
/// tests use [`RamFlashWriter`].
pub trait FlashWriter {
    /// Write `data` at the absolute `offset` within the image.
    fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), NodeError>;

    /// Verify the written image's CRC32 against `expected`.
    fn verify(&self, expected: u32) -> Result<bool, NodeError>;

    /// Returns the raw image bytes written so far.
    fn as_bytes(&self) -> &[u8];
}

/// In-RAM flash writer. Uses a fixed-capacity `heapless::Vec`, so it works in
/// both `no_std` (embedded) and `std` (host test) configurations.
pub struct RamFlashWriter {
    buffer: heapless::Vec<u8, MAX_OTA_BUFFER_SIZE>,
}

impl RamFlashWriter {
    pub fn new() -> Self {
        Self {
            buffer: heapless::Vec::new(),
        }
    }

    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }
}

impl Default for RamFlashWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl FlashWriter for RamFlashWriter {
    fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), NodeError> {
        let start = offset as usize;
        let end = start + data.len();
        if end > self.buffer.capacity() {
            return Err(NodeError::OtaFailed);
        }
        if self.buffer.len() < end {
            self.buffer
                .resize(end, 0)
                .map_err(|_| NodeError::OtaFailed)?;
        }
        self.buffer[start..end].copy_from_slice(data);
        Ok(())
    }

    fn verify(&self, expected: u32) -> Result<bool, NodeError> {
        let actual = crc32(&self.buffer);
        Ok(actual == expected)
    }

    fn as_bytes(&self) -> &[u8] {
        &self.buffer
    }
}

/// CRC32 (ISO-HDLC polynomial) — same algorithm used by `tpt-e-link`'s framing
/// layer, so the host and device agree on the checksum.
fn crc32(data: &[u8]) -> u32 {
    const CRC: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);
    CRC.checksum(data)
}

/// Internal state machine for the OTA handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OtaState {
    /// No OTA update in progress.
    Idle,
    /// Receiving chunks. `received` tracks total bytes written so far.
    Receiving { received: u32 },
    /// All chunks received; awaiting CRC verification.
    Verifying,
}

/// Automatic OTA handler: receives chunked firmware images, writes them to
/// flash via a [`FlashWriter`], and verifies the CRC32 on completion.
pub struct OtaHandler {
    state: OtaState,
    writer: Box<dyn FlashWriter + Send>,
    expected_crc: u32,
    expected_size: u32,
}

impl fmt::Debug for OtaHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OtaHandler")
            .field("state", &self.state)
            .field("expected_crc", &self.expected_crc)
            .field("expected_size", &self.expected_size)
            .finish_non_exhaustive()
    }
}

impl OtaHandler {
    /// Creates a new OTA handler backed by the given [`FlashWriter`].
    pub fn new(writer: Box<dyn FlashWriter + Send>) -> Self {
        Self {
            state: OtaState::Idle,
            writer,
            expected_crc: 0,
            expected_size: 0,
        }
    }

    /// Returns `true` if an OTA update is currently in progress.
    pub fn is_active(&self) -> bool {
        !matches!(self.state, OtaState::Idle)
    }

    /// Resets the handler to the idle state.
    pub fn reset(&mut self) {
        self.state = OtaState::Idle;
        self.expected_crc = 0;
        self.expected_size = 0;
    }

    /// Parses `OtaStart` args: `[expected_size: u32][expected_crc: u32]`.
    fn parse_start(args: &[u8]) -> Result<(u32, u32), NodeError> {
        if args.len() < 8 {
            return Err(NodeError::OtaFailed);
        }
        let size = u32::from_le_bytes([args[0], args[1], args[2], args[3]]);
        let crc = u32::from_le_bytes([args[4], args[5], args[6], args[7]]);
        Ok((size, crc))
    }

    /// Parses `OtaChunk` args: `[offset: u32][data: [u8; …]]`.
    fn parse_chunk(args: &[u8]) -> Result<(u32, &[u8]), NodeError> {
        if args.len() < 4 {
            return Err(NodeError::OtaFailed);
        }
        let offset = u32::from_le_bytes([args[0], args[1], args[2], args[3]]);
        Ok((offset, &args[4..]))
    }
}

impl crate::CommandHandler for OtaHandler {
    fn handles(&self, command_id: u32) -> bool {
        let base = command_id & 0xFF00;
        base == cmd::OTA_BASE
    }

    fn handle(&mut self, cmd: &CommandPayload) -> AckPayload {
        match cmd.command_id {
            cmd::OTA_START => match Self::parse_start(&cmd.args) {
                Ok((size, crc)) => {
                    self.expected_size = size;
                    self.expected_crc = crc;
                    self.state = OtaState::Receiving { received: 0 };
                    AckPayload {
                        command_id: cmd.command_id,
                        success: true,
                        error_code: 0,
                    }
                }
                Err(_) => AckPayload {
                    command_id: cmd.command_id,
                    success: false,
                    error_code: 1,
                },
            },

            cmd::OTA_CHUNK => {
                if !matches!(self.state, OtaState::Receiving { .. }) {
                    return AckPayload {
                        command_id: cmd.command_id,
                        success: false,
                        error_code: 2,
                    };
                }
                match Self::parse_chunk(&cmd.args) {
                    Ok((offset, data)) => {
                        if self.writer.write(offset, data).is_err() {
                            return AckPayload {
                                command_id: cmd.command_id,
                                success: false,
                                error_code: 3,
                            };
                        }
                        if let OtaState::Receiving { received } = &mut self.state {
                            *received += data.len() as u32;
                        }
                        AckPayload {
                            command_id: cmd.command_id,
                            success: true,
                            error_code: 0,
                        }
                    }
                    Err(_) => AckPayload {
                        command_id: cmd.command_id,
                        success: false,
                        error_code: 1,
                    },
                }
            }

            cmd::OTA_FINISH => {
                if !matches!(self.state, OtaState::Receiving { .. }) {
                    return AckPayload {
                        command_id: cmd.command_id,
                        success: false,
                        error_code: 2,
                    };
                }
                self.state = OtaState::Verifying;
                let crc_ok = self.writer.verify(self.expected_crc).unwrap_or(false);
                if crc_ok {
                    self.state = OtaState::Idle;
                    AckPayload {
                        command_id: cmd.command_id,
                        success: true,
                        error_code: 0,
                    }
                } else {
                    self.reset();
                    AckPayload {
                        command_id: cmd.command_id,
                        success: false,
                        error_code: 4,
                    }
                }
            }

            _ => AckPayload {
                command_id: cmd.command_id,
                success: false,
                error_code: 0,
            },
        }
    }
}

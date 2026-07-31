//! Fake, host-backed capability drivers for `tpt-node-mock`.
//!
//! ## LittleFS wire format (mock-local interpretation)
//!
//! The wire protocol's `CommandPayload` is generic; the mock defines a
//! compact argument layout for the `0x01xx` LittleFS command range:
//!
//! - All commands: `[path_len: u8][path: path_len bytes][extra: …]`
//! - `LITTLEFS_READ`  (0x0100): `extra = [offset: u32 LE]`
//! - `LITTLEFS_WRITE` (0x0101): `extra = [offset: u32 LE][data: …]`
//! - `LITTLEFS_LIST`  (0x0102): `extra = []`
//! - `LITTLEFS_FORMAT`(0x0103): `extra = []` (no-op on a host directory)
//!
//! Paths are resolved relative to the mock's root directory, with traversal
//! outside the root rejected.

use std::path::{Path, PathBuf};

use heapless::Vec;
use tpt_e_link::message::{AckPayload, CommandPayload, TelemetrySample};
use tpt_node_core::{cmd, CommandHandler, LittleFsDriver, NodeError};

/// A simulated LittleFS backed by a real directory on the host's hard drive.
///
/// `read_file`/`write_file`/`list_dir` map 1:1 onto `std::fs` operations
/// under [`root`](Self::root). Path traversal outside `root` is rejected.
pub struct HostLittleFsDriver {
    root: PathBuf,
}

impl HostLittleFsDriver {
    /// Creates a driver rooted at `root` (created if missing by the binary).
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Returns the backing directory on the host.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn resolve(&self, path: &str) -> Result<PathBuf, NodeError> {
        let relative = Path::new(path);
        if relative.is_absolute() {
            return Err(NodeError::InvalidState);
        }
        for component in relative.components() {
            if matches!(component, std::path::Component::ParentDir) {
                return Err(NodeError::InvalidState);
            }
        }
        Ok(self.root.join(relative))
    }
}

/// Splits `[path_len: u8][path][extra]` into the decoded path and the
/// remaining bytes.
fn decode_path(args: &[u8]) -> Result<(String, &[u8]), NodeError> {
    if args.is_empty() {
        return Err(NodeError::InvalidState);
    }
    let len = args[0] as usize;
    if len == 0 || len > args.len() - 1 {
        return Err(NodeError::InvalidState);
    }
    let path = std::str::from_utf8(&args[1..1 + len]).map_err(|_| NodeError::InvalidState)?;
    Ok((path.to_string(), &args[1 + len..]))
}

impl CommandHandler for HostLittleFsDriver {
    fn handles(&self, command_id: u32) -> bool {
        (cmd::LITTLEFS_BASE..cmd::LITTLEFS_BASE + 0x100).contains(&command_id)
    }

    fn handle(&mut self, cmd: &CommandPayload) -> AckPayload {
        let result = match cmd.command_id {
            cmd::LITTLEFS_LIST => match decode_path(&cmd.args) {
                Ok((path, _)) => self.list_dir(&path).map(|_| ()),
                Err(e) => Err(e),
            },
            cmd::LITTLEFS_READ => {
                let read = (|| -> Result<usize, NodeError> {
                    let (path, rest) = decode_path(&cmd.args)?;
                    if rest.len() < 4 {
                        return Err(NodeError::InvalidState);
                    }
                    let offset = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]);
                    let mut buf = [0u8; 512];
                    self.read_file(&path, offset, &mut buf)
                })();
                read.map(|_| ())
            }
            cmd::LITTLEFS_WRITE => {
                let write = (|| -> Result<(), NodeError> {
                    let (path, rest) = decode_path(&cmd.args)?;
                    if rest.len() < 4 {
                        return Err(NodeError::InvalidState);
                    }
                    let offset = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]);
                    self.write_file(&path, offset, &rest[4..])
                })();
                write
            }
            cmd::LITTLEFS_FORMAT => Ok(()),
            _ => Err(NodeError::InvalidState),
        };

        match result {
            Ok(()) => AckPayload {
                command_id: cmd.command_id,
                success: true,
                error_code: 0,
            },
            Err(_) => AckPayload {
                command_id: cmd.command_id,
                success: false,
                error_code: 1,
            },
        }
    }
}

impl LittleFsDriver for HostLittleFsDriver {
    fn read_file(&mut self, path: &str, offset: u32, buf: &mut [u8]) -> Result<usize, NodeError> {
        use std::io::{Read, Seek, SeekFrom};
        let path = self.resolve(path)?;
        let mut file = std::fs::File::open(&path).map_err(|_| NodeError::Io)?;
        file.seek(SeekFrom::Start(offset as u64))
            .map_err(|_| NodeError::Io)?;
        file.read(buf).map_err(|_| NodeError::Io)
    }

    fn write_file(&mut self, path: &str, offset: u32, data: &[u8]) -> Result<(), NodeError> {
        use std::io::{Seek, SeekFrom, Write};
        let path = self.resolve(path)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| NodeError::Io)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&path)
            .map_err(|_| NodeError::Io)?;
        file.seek(SeekFrom::Start(offset as u64))
            .map_err(|_| NodeError::Io)?;
        file.write_all(data).map_err(|_| NodeError::Io)
    }

    fn list_dir(
        &mut self,
        path: &str,
    ) -> Result<Vec<heapless::String<64>, 32>, NodeError> {
        let path = self.resolve(path)?;
        let mut out: Vec<heapless::String<64>, 32> = Vec::new();
        for entry in std::fs::read_dir(&path).map_err(|_| NodeError::Io)? {
            let entry = entry.map_err(|_| NodeError::Io)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let mut s: heapless::String<64> = heapless::String::new();
            // Truncate names longer than 63 bytes so a long filename can't
            // fail the whole listing.
            let trunc = name.chars().take(63).collect::<String>();
            let _ = s.push_str(&trunc);
            if out.push(s).is_err() {
                break;
            }
        }
        Ok(out)
    }
}

/// A simulated telemetry driver producing a couple of fake sensor readings.
pub struct MockTelemetryDriver;

impl CommandHandler for MockTelemetryDriver {
    fn handles(&self, command_id: u32) -> bool {
        (cmd::TELEMETRY_BASE..cmd::TELEMETRY_BASE + 0x100).contains(&command_id)
    }

    fn handle(&mut self, cmd: &CommandPayload) -> AckPayload {
        AckPayload {
            command_id: cmd.command_id,
            success: true,
            error_code: 0,
        }
    }
}

impl tpt_node_core::TelemetryDriver for MockTelemetryDriver {
    fn read_samples(&mut self) -> Vec<TelemetrySample, { tpt_e_link::message::MAX_TELEMETRY_SAMPLES }> {
        let mut samples = Vec::new();
        let _ = samples.push(TelemetrySample {
            sensor_id: 1,
            value: 21.5,
        });
        let _ = samples.push(TelemetrySample {
            sensor_id: 2,
            value: 48.2,
        });
        samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> (tempfile::TempDir, HostLittleFsDriver) {
        let dir = tempfile::tempdir().expect("tempdir");
        let driver = HostLittleFsDriver::new(dir.path().to_path_buf());
        (dir, driver)
    }

    #[test]
    fn read_write_round_trip_on_host_fs() {
        let (_dir, mut driver) = temp_root();
        driver
            .write_file("hello.txt", 0, b"hello mock fs")
            .expect("write");
        let mut buf = [0u8; 32];
        let n = driver
            .read_file("hello.txt", 0, &mut buf)
            .expect("read");
        assert_eq!(&buf[..n], b"hello mock fs");
    }

    #[test]
    fn write_at_offset_pads_via_seek() {
        let (_dir, mut driver) = temp_root();
        driver.write_file("a.txt", 4, b"X").expect("write");
        let mut buf = [0u8; 16];
        let n = driver.read_file("a.txt", 0, &mut buf).expect("read");
        // Bytes 0..4 are holes (read as zero), byte 4 is 'X'.
        assert_eq!(&buf[..n], &[0, 0, 0, 0, b'X']);
    }

    #[test]
    fn list_dir_returns_entries() {
        let (_dir, mut driver) = temp_root();
        driver.write_file("one.txt", 0, b"1").expect("write");
        driver.write_file("two.txt", 0, b"2").expect("write");
        let entries = driver.list_dir(".").expect("list");
        assert_eq!(entries.len(), 2);
        let names: std::vec::Vec<String> = entries.iter().map(|s| s.to_string()).collect();
        assert!(names.contains(&"one.txt".to_string()));
        assert!(names.contains(&"two.txt".to_string()));
    }

    #[test]
    fn path_traversal_is_rejected() {
        let (_dir, mut driver) = temp_root();
        driver.write_file("ok.txt", 0, b"x").expect("write");
        assert!(driver.read_file("../escape.txt", 0, &mut [0u8; 4]).is_err());
        assert!(driver.list_dir("../").is_err());
    }

    #[test]
    fn decode_path_parses_layout() {
        let args = [3u8, b'a', b'b', b'c', 1, 2, 3, 4];
        let (path, rest) = decode_path(&args).expect("decode");
        assert_eq!(path, "abc");
        assert_eq!(rest, &[1, 2, 3, 4]);
    }
}

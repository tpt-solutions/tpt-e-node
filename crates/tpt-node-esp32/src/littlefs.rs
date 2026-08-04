//! LittleFS bindings over a flash region.
//!
//! [`EspLittleFsDriver`] bridges `littlefs2` to an `embedded-storage`
//! [`NorFlash`] device so the `LittleFS` capability of `tpt-node-core` runs on
//! real flash. It implements:
//!
//! - [`littlefs2::driver::Storage`] for [`RegionStorage`] — each storage call
//!   maps to an aligned `embedded-storage` call, and the region's
//!   `[BASE, BASE + LEN)` partition defines the file-system geometry
//!   (`BLOCK_COUNT = LEN / ERASE_SIZE`).
//! - [`LittleFsDriver`] / [`CommandHandler`] — the wire-facing API used by
//!   `tpt-node-core`'s capability registry.
//!
//! # Wire layout (matches `tpt-node-mock`'s LittleFS driver)
//!
//! | Command          | ID       | Args (LE)                                   |
//! |------------------|----------|---------------------------------------------|
//! | `LittleFsList`   | `0x0100` | `path_len: u8` + `path`                     |
//! | `LittleFsRead`   | `0x0101` | `path_len: u8` + `path` + `offset: u32`     |
//! | `LittleFsWrite`  | `0x0102` | `path_len: u8` + `path` + `offset: u32` + data |
//! | `LittleFsFormat` | `0x0103` | *(empty)*                                   |
//!
//! Paths are mounted relative to the file-system root; `list_dir` returns up to
//! 32 names truncated to 64 bytes each, matching `tpt-node-core`'s
//! `LittleFsDriver::list_dir` signature.

use core::fmt;

use embedded_storage::nor_flash::{NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash};
use heapless::Vec;
use littlefs2::consts::{U64, U8};
use littlefs2::driver::Storage as LfsStorage;
use littlefs2::fs::Filesystem;
use littlefs2::io::{Error as LfsError, SeekFrom, Write};
use littlefs2::path::PathBuf;
use tpt_e_link::message::{AckPayload, CommandPayload};
use tpt_node_core::{cmd, CommandHandler, LittleFsDriver, NodeError};

use crate::flash::RegionStorage;

/// Number of file names `list_dir` can return in one reply.
const MAX_LIST_ENTRIES: usize = 32;

/// Maps an `embedded-storage` flash error onto a LittleFS error kind.
fn lfs_err<E: NorFlashError>(err: E) -> LfsError {
    match err.kind() {
        NorFlashErrorKind::NotAligned => LfsError::INVALID,
        NorFlashErrorKind::OutOfBounds => LfsError::IO,
        _ => LfsError::CORRUPTION,
    }
}

/// Maps a LittleFS error onto a `tpt-node-core` error.
fn node_err(_err: LfsError) -> NodeError {
    NodeError::Io
}

impl<S, const BASE: u32, const LEN: u32> LfsStorage for RegionStorage<S, BASE, LEN>
where
    S: NorFlash + ReadNorFlash,
    S::Error: NorFlashError,
{
    const READ_SIZE: usize = S::READ_SIZE;
    const WRITE_SIZE: usize = S::WRITE_SIZE;
    const BLOCK_SIZE: usize = S::ERASE_SIZE;
    const BLOCK_COUNT: usize = LEN as usize / S::ERASE_SIZE;
    const BLOCK_CYCLES: isize = 64;
    type CACHE_SIZE = U64;
    type LOOKAHEAD_SIZE = U8;

    fn read(&mut self, off: usize, buf: &mut [u8]) -> Result<usize, LfsError> {
        <Self as ReadNorFlash>::read(self, off as u32, buf).map_err(lfs_err)?;
        Ok(buf.len())
    }

    fn write(&mut self, off: usize, data: &[u8]) -> Result<usize, LfsError> {
        <Self as NorFlash>::write(self, off as u32, data).map_err(lfs_err)?;
        Ok(data.len())
    }

    fn erase(&mut self, off: usize, len: usize) -> Result<usize, LfsError> {
        <Self as NorFlash>::erase(self, off as u32, (off + len) as u32).map_err(lfs_err)?;
        Ok(len)
    }
}

/// A LittleFS file system backed by `[BASE, BASE + LEN)` of a flash device.
pub struct EspLittleFsDriver<S, const BASE: u32, const LEN: u32> {
    storage: RegionStorage<S, BASE, LEN>,
}

impl<S, const BASE: u32, const LEN: u32> EspLittleFsDriver<S, BASE, LEN>
where
    S: NorFlash + ReadNorFlash,
    S::Error: NorFlashError,
{
    /// Creates a driver over `[BASE, BASE + LEN)` of `storage`.
    pub fn new(storage: S) -> Self {
        Self {
            storage: RegionStorage::new(storage),
        }
    }

    /// Formats (re-initializes) the file system, erasing the whole region.
    pub fn format(&mut self) -> Result<(), NodeError> {
        Filesystem::format(&mut self.storage).map_err(node_err)?;
        Ok(())
    }

    /// The region backing this driver.
    pub fn region(&self) -> &RegionStorage<S, BASE, LEN> {
        &self.storage
    }

    /// Consumes the driver and returns the underlying storage.
    pub fn into_inner(self) -> S {
        self.storage.into_inner()
    }

    /// Mounts the file system and runs `f` against it.
    fn with_fs<R>(
        &mut self,
        f: impl FnOnce(&Filesystem<RegionStorage<S, BASE, LEN>>) -> Result<R, LfsError>,
    ) -> Result<R, NodeError> {
        Filesystem::mount_and_then(&mut self.storage, f).map_err(node_err)
    }
}

impl<S, const BASE: u32, const LEN: u32> fmt::Debug for EspLittleFsDriver<S, BASE, LEN> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EspLittleFsDriver")
            .field("region", &self.storage)
            .finish_non_exhaustive()
    }
}

impl<S, const BASE: u32, const LEN: u32> LittleFsDriver for EspLittleFsDriver<S, BASE, LEN>
where
    S: NorFlash + ReadNorFlash,
    S::Error: NorFlashError,
{
    fn read_file(&mut self, path: &str, offset: u32, buf: &mut [u8]) -> Result<usize, NodeError> {
        let path = PathBuf::try_from(path).map_err(|_| NodeError::InvalidState)?;
        self.with_fs(|fs| {
            fs.open_file_and_then(&path, |file| {
                file.seek(SeekFrom::Start(offset))?;
                file.read(buf)
            })
        })
    }

    fn write_file(&mut self, path: &str, offset: u32, data: &[u8]) -> Result<(), NodeError> {
        let path = PathBuf::try_from(path).map_err(|_| NodeError::InvalidState)?;
        self.with_fs(|fs| {
            fs.open_file_with_options_and_then(
                |options| options.create(true).write(true),
                &path,
                |file| {
                    file.seek(SeekFrom::Start(offset))?;
                    file.write_all(data)
                },
            )
        })?;
        Ok(())
    }

    fn list_dir(&mut self, path: &str) -> Result<Vec<heapless::String<64>, 32>, NodeError> {
        let path = PathBuf::try_from(path).map_err(|_| NodeError::InvalidState)?;
        let entries = self.with_fs(|fs| {
            fs.read_dir_and_then(&path, |read_dir| {
                let mut out: Vec<heapless::String<64>, 32> = Vec::new();
                for entry in read_dir {
                    let entry = entry?;
                    let mut name: heapless::String<64> = heapless::String::new();
                    let _ = name.push_str(entry.file_name().as_str());
                    if out.push(name).is_err() {
                        break;
                    }
                }
                Ok(out)
            })
        })?;
        Ok(entries)
    }
}

impl<S, const BASE: u32, const LEN: u32> CommandHandler for EspLittleFsDriver<S, BASE, LEN>
where
    S: NorFlash + ReadNorFlash,
    S::Error: NorFlashError,
{
    fn handles(&self, command_id: u32) -> bool {
        (cmd::LITTLEFS_BASE..cmd::LITTLEFS_BASE + 0x100).contains(&command_id)
    }

    fn handle(&mut self, command: &CommandPayload) -> AckPayload {
        let result = match command.command_id {
            cmd::LITTLEFS_LIST => decode_path(&command.args)
                .and_then(|(path, _)| self.list_dir(&path).map(|_| ())),
            cmd::LITTLEFS_READ => decode_path(&command.args).and_then(|(path, rest)| {
                let offset = decode_u32(rest)?;
                let mut buf = [0u8; 512];
                self.read_file(&path, offset, &mut buf).map(|_| ())
            }),
            cmd::LITTLEFS_WRITE => decode_path(&command.args).and_then(|(path, rest)| {
                let offset = decode_u32(rest)?;
                self.write_file(&path, offset, &rest[4..])
            }),
            cmd::LITTLEFS_FORMAT => self.format(),
            _ => Err(NodeError::InvalidState),
        };
        match result {
            Ok(()) => AckPayload {
                command_id: command.command_id,
                success: true,
                error_code: 0,
            },
            Err(_) => AckPayload {
                command_id: command.command_id,
                success: false,
                error_code: 1,
            },
        }
    }
}

/// Decodes `path_len: u8 + path` from the start of `args`, returning the path
/// and the remaining argument bytes.
fn decode_path(args: &[u8]) -> Result<(heapless::String<256>, &[u8]), NodeError> {
    let len = *args.first().ok_or(NodeError::InvalidState)? as usize;
    if len == 0 || len > args.len().saturating_sub(1) {
        return Err(NodeError::InvalidState);
    }
    let path = core::str::from_utf8(&args[1..1 + len]).map_err(|_| NodeError::InvalidState)?;
    let mut out: heapless::String<256> = heapless::String::new();
    out.push_str(path).map_err(|_| NodeError::InvalidState)?;
    Ok((out, &args[1 + len..]))
}

/// Decodes a little-endian `u32` from the start of `rest`.
fn decode_u32(rest: &[u8]) -> Result<u32, NodeError> {
    let bytes: [u8; 4] = rest[..4].try_into().map_err(|_| NodeError::InvalidState)?;
    Ok(u32::from_le_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::MockFlash;

    const FS_BASE: u32 = 0;
    const FS_LEN: u32 = 128 * 1024;

    type Driver = EspLittleFsDriver<MockFlash, FS_BASE, FS_LEN>;

    fn new_formatted() -> Driver {
        let mut driver = Driver::new(MockFlash::new(FS_LEN as usize + 4096));
        driver.format().expect("format");
        driver
    }

    fn encode_path(path: &str) -> Vec<u8, 32> {
        let mut args: Vec<u8, 32> = Vec::new();
        args.push(path.len() as u8).expect("push");
        args.extend_from_slice(path.as_bytes()).expect("push");
        args
    }

    fn write_args(path: &str, offset: u32, data: &[u8]) -> Vec<u8, 32> {
        let mut args = encode_path(path);
        args.extend_from_slice(&offset.to_le_bytes()).expect("push");
        args.extend_from_slice(data).expect("push");
        args
    }

    #[test]
    fn format_creates_mountable_fs() {
        let mut driver = new_formatted();
        let entries = driver.list_dir("/").expect("list");
        assert!(entries.is_empty());
    }

    #[test]
    fn write_read_roundtrip() {
        let mut driver = new_formatted();
        driver.write_file("/hello.txt", 0, b"hello world").expect("write");

        let mut buf = [0u8; 64];
        let n = driver.read_file("/hello.txt", 0, &mut buf).expect("read");
        assert_eq!(n, 11);
        assert_eq!(&buf[..n], b"hello world");
    }

    #[test]
    fn write_at_offset_pads() {
        let mut driver = new_formatted();
        driver.write_file("/padded.bin", 4, b"XY").expect("write");

        let mut buf = [0u8; 8];
        let n = driver.read_file("/padded.bin", 0, &mut buf).expect("read");
        assert_eq!(n, 6);
        assert_eq!(&buf[..6], &[0, 0, 0, 0, b'X', b'Y']);
    }

    #[test]
    fn overwrite_preserves_tail() {
        let mut driver = new_formatted();
        driver.write_file("/tail.txt", 0, b"abcdef").expect("write");
        driver.write_file("/tail.txt", 3, b"XY").expect("write");

        let mut buf = [0u8; 8];
        let n = driver.read_file("/tail.txt", 0, &mut buf).expect("read");
        assert_eq!(n, 6);
        assert_eq!(&buf[..6], b"abcXYf");
    }

    #[test]
    fn list_dir_lists_names() {
        let mut driver = new_formatted();
        driver.write_file("/alpha", 0, b"1").expect("write");
        driver.write_file("/beta", 0, b"2").expect("write");

        let entries = driver.list_dir("/").expect("list");
        assert_eq!(entries.len(), 2);
        let names: Vec<heapless::String<64>, 4> =
            entries.iter().map(|s| s.clone()).collect();
        assert!(names.iter().any(|n| n == "/alpha"));
        assert!(names.iter().any(|n| n == "/beta"));
    }

    #[test]
    fn command_handler_wire_roundtrip() {
        let mut driver = new_formatted();

        let write_cmd = CommandPayload {
            command_id: cmd::LITTLEFS_WRITE,
            args: write_args("/wire.txt", 0, b"payload").into(),
        };
        let ack = CommandHandler::handle(&mut driver, &write_cmd);
        assert!(ack.success, "ack: {ack:?}");

        let read_cmd = CommandPayload {
            command_id: cmd::LITTLEFS_READ,
            args: write_args("/wire.txt", 0, &[]).into(),
        };
        let ack = CommandHandler::handle(&mut driver, &read_cmd);
        assert!(ack.success, "ack: {ack:?}");
    }

    #[test]
    fn command_handler_rejects_bad_path() {
        let mut driver = new_formatted();
        let cmd = CommandPayload {
            command_id: cmd::LITTLEFS_WRITE,
            args: encode_path("").into(),
        };
        let ack = CommandHandler::handle(&mut driver, &cmd);
        assert!(!ack.success);
    }
}

#[test]
fn debug_print_fresh_entries() {
    let mut driver = new_formatted();
    let entries = driver.list_dir("/").expect("list");
    eprintln!("DEBUG fresh root entries = {:?}", entries);
    for e in entries.iter() {
        eprintln!("DEBUG name bytes = {:?}", e.as_bytes());
    }
    driver.write_file("/a", 0, b"x").expect("w");
    let entries = driver.list_dir("/").expect("list");
    eprintln!("DEBUG after write = {:?}", entries);
}
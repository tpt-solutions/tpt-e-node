//! Flash-backed storage regions and a streamed OTA image writer.
//!
//! The ESP32 does not expose flash via `esp-hal` 0.22; the `esp-storage` crate
//! provides an `embedded-storage` [`NorFlash`] driver for the SPI flash. This
//! module layers two things on top of that:
//!
//! - [`RegionStorage`] — a compile-time `[BASE, BASE + LEN)` view over any
//!   `embedded-storage` device, used to carve an OTA partition and a LittleFS
//!   partition out of the same flash chip.
//! - [`OtaRegionFlashWriter`] — a [`tpt_node_core::FlashWriter`] that streams
//!   OTA chunks straight to flash (word-aligned, sector-erased) while
//!   maintaining an incremental CRC32, so images are verified on `OtaFinish`
//!   *without* buffering the whole image in RAM.
//!
//! # Write semantics
//!
//! NOR flash can only clear bits (`1 → 0`), so a region must be erased before
//! it is programmed. The OTA writer erases the entire region lazily on the
//! first write of a session (identified by `offset == 0`, per the OTA
//! protocol's chunk stream) and preserves partial-word bytes at chunk
//! boundaries by reading the adjacent flash word. Chunks are assumed to arrive
//! in ascending `offset` order with no out-of-session overlap, which is how
//! `tpt-node-core`'s OTA handler drives the writer.

use core::fmt;

use embedded_storage::nor_flash::{
    ErrorType, MultiwriteNorFlash, NorFlash, ReadNorFlash,
};
use tpt_node_core::{FlashWriter, NodeError};

use crate::crc::IsoHdlcCrc32;

/// Default OTA partition size the writer is sized for (256 KiB).
pub const OTA_REGION_SIZE: u32 = 256 * 1024;

/// Maximum flash write window (one chunk rounded up to the write-word size
/// plus a partial head word). 256 bytes comfortably covers the 28-byte OTA
/// chunks for any word size up to ~128 bytes.
const MAX_FLASH_WINDOW: usize = 256;

/// Rounds `v` up to the next multiple of `a`.
const fn align_up(v: u32, a: u32) -> u32 {
    v.div_ceil(a) * a
}

/// A compile-time `[BASE, BASE + LEN)` sub-region view over a `NorFlash`
/// device.
///
/// `BASE` must be aligned to `S::WRITE_SIZE` and `LEN` should be a multiple of
/// `S::ERASE_SIZE` (drivers such as LittleFS use `LEN / S::ERASE_SIZE` as the
/// block count). Out-of-region accesses are `debug_assert!`-checked only, so
/// callers must construct the region with a correct partition table.
pub struct RegionStorage<S, const BASE: u32, const LEN: u32> {
    storage: S,
}

impl<S, const BASE: u32, const LEN: u32> RegionStorage<S, BASE, LEN> {
    /// Wraps `storage`, exposing only the `[BASE, BASE + LEN)` byte range.
    pub const fn new(storage: S) -> Self {
        Self { storage }
    }

    /// Absolute start offset of this region in the underlying device.
    pub const fn base(&self) -> u32 {
        BASE
    }

    /// Length of this region in bytes.
    pub const fn len(&self) -> u32 {
        LEN
    }

    /// Whether this region is empty (zero length).
    pub const fn is_empty(&self) -> bool {
        LEN == 0
    }

    /// Consumes the wrapper and returns the underlying storage.
    pub fn into_inner(self) -> S {
        self.storage
    }
}

impl<S, const BASE: u32, const LEN: u32> fmt::Debug for RegionStorage<S, BASE, LEN> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegionStorage")
            .field("base", &BASE)
            .field("len", &LEN)
            .finish_non_exhaustive()
    }
}

impl<S, const BASE: u32, const LEN: u32> ErrorType for RegionStorage<S, BASE, LEN>
where
    S: ErrorType,
{
    type Error = S::Error;
}

impl<S, const BASE: u32, const LEN: u32> ReadNorFlash for RegionStorage<S, BASE, LEN>
where
    S: ReadNorFlash,
{
    const READ_SIZE: usize = S::READ_SIZE;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        debug_assert!(
            offset
                .checked_add(bytes.len() as u32)
                .is_some_and(|end| end <= LEN),
            "read outside region [{BASE}, {})",
            BASE + LEN
        );
        self.storage.read(BASE + offset, bytes)
    }

    fn capacity(&self) -> usize {
        LEN as usize
    }
}

impl<S, const BASE: u32, const LEN: u32> NorFlash for RegionStorage<S, BASE, LEN>
where
    S: NorFlash,
{
    const WRITE_SIZE: usize = S::WRITE_SIZE;
    const ERASE_SIZE: usize = S::ERASE_SIZE;

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        debug_assert!(
            offset
                .checked_add(bytes.len() as u32)
                .is_some_and(|end| end <= LEN),
            "write outside region [{BASE}, {})",
            BASE + LEN
        );
        self.storage.write(BASE + offset, bytes)
    }

    fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        debug_assert!(from <= to && to <= LEN, "erase outside region [{BASE}, {})", BASE + LEN);
        self.storage.erase(BASE + from, BASE + to)
    }
}

impl<S, const BASE: u32, const LEN: u32> MultiwriteNorFlash for RegionStorage<S, BASE, LEN> where
    S: MultiwriteNorFlash
{
}

/// A [`tpt_node_core::FlashWriter`] that streams an OTA image into a flash
/// region, verifying the image by incremental CRC on [`verify`](Self::verify).
///
/// `S` is the underlying `NorFlash` device (e.g.
/// `esp_storage::FlashStorage`); `BASE`/`LEN` define the OTA partition. On a
/// new session (`offset == 0`) the writer re-erases the region and resets its
/// CRC, so repeated OTA attempts over the device lifetime behave correctly.
pub struct OtaRegionFlashWriter<S, const BASE: u32, const LEN: u32> {
    region: RegionStorage<S, BASE, LEN>,
    written: u32,
    crc: IsoHdlcCrc32,
    erased: bool,
}

impl<S, const BASE: u32, const LEN: u32> OtaRegionFlashWriter<S, BASE, LEN>
where
    S: NorFlash + ReadNorFlash,
{
    /// Creates a writer targeting `[BASE, BASE + LEN)` of `storage`.
    pub fn new(storage: S) -> Self {
        Self {
            region: RegionStorage::new(storage),
            written: 0,
            crc: IsoHdlcCrc32::new(),
            erased: false,
        }
    }

    /// Total bytes streamed into flash in the current session.
    pub fn written(&self) -> u32 {
        self.written
    }

    /// Running CRC-32/ISO-HDLC of everything streamed this session.
    pub fn running_crc(&self) -> u32 {
        self.crc.finalize()
    }

    /// The region backing this writer.
    pub fn region(&self) -> &RegionStorage<S, BASE, LEN> {
        &self.region
    }

    /// Consumes the writer and returns the underlying storage.
    pub fn into_inner(self) -> S {
        self.region.into_inner()
    }

    fn erase_region(&mut self) -> Result<(), NodeError> {
        self.region
            .storage
            .erase(BASE, BASE + LEN)
            .map_err(|_| NodeError::Io)
    }

    /// Writes one (possibly partial-word) chunk, preserving the head word.
    fn write_flash(&mut self, offset: u32, data: &[u8]) -> Result<(), NodeError> {
        let word = S::WRITE_SIZE as u32;
        let start = offset - offset % word;
        let rem = (offset + data.len() as u32) % word;
        let end = offset + data.len() as u32 + if rem == 0 { 0 } else { word - rem };
        let window = (end - start) as usize;
        if window > MAX_FLASH_WINDOW {
            return Err(NodeError::OtaFailed);
        }

        let mut buf = [0xFFu8; MAX_FLASH_WINDOW];
        if offset > start {
            // Preserve the partial word that precedes the chunk.
            let head_len = align_up(word, S::READ_SIZE as u32) as usize;
            let mut head = [0u8; MAX_FLASH_WINDOW];
            self.region
                .storage
                .read(BASE + start, &mut head[..head_len])
                .map_err(|_| NodeError::Io)?;
            buf[..(offset - start) as usize].copy_from_slice(&head[..(offset - start) as usize]);
        }
        buf[(offset - start) as usize..(offset - start + data.len() as u32) as usize]
            .copy_from_slice(data);

        self.region
            .storage
            .write(BASE + start, &buf[..window])
            .map_err(|_| NodeError::Io)
    }
}

impl<S, const BASE: u32, const LEN: u32> FlashWriter for OtaRegionFlashWriter<S, BASE, LEN>
where
    S: NorFlash + ReadNorFlash,
{
    fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), NodeError> {
        if data.is_empty() {
            return Ok(());
        }
        let end_offset = offset
            .checked_add(data.len() as u32)
            .ok_or(NodeError::OtaFailed)?;
        if end_offset > LEN {
            return Err(NodeError::OtaFailed);
        }

        // A write at offset 0 starts a new session (per the OTA protocol's
        // chunk stream): reset the incremental CRC and re-erase lazily.
        if offset == 0 {
            self.crc.reset();
            self.written = 0;
            self.erased = false;
        }
        if !self.erased {
            self.erase_region()?;
            self.erased = true;
        }

        self.crc.update(data);
        self.write_flash(offset, data)?;
        self.written = self.written.max(end_offset);
        Ok(())
    }

    fn verify(&self, expected: u32) -> Result<bool, NodeError> {
        Ok(self.crc.finalize() == expected)
    }

    fn as_bytes(&self) -> &[u8] {
        // Images live in flash, not RAM: there is no in-memory copy to hand
        // out. The OTA handler uses `verify`, not `as_bytes`.
        &[]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use crate::test_util::MockFlash;

    type OtaWriter = OtaRegionFlashWriter<MockFlash, 4096, 4096>;

    fn reference_crc(image: &[u8]) -> u32 {
        crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC).checksum(image)
    }

    #[test]
    fn region_read_write_roundtrip() {
        let mut region = RegionStorage::<MockFlash, 0, 4096>::new(MockFlash::new(8192));
        region.erase(0, 4096).expect("erase");
        region.write(0, &[1, 2, 3, 4, 5, 6, 7, 8]).expect("write");
        let mut buf = [0u8; 8];
        region.read(0, &mut buf).expect("read");
        assert_eq!(&buf, &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(region.capacity(), 4096);
    }

    #[test]
    fn region_passthrough_offsets() {
        // BASE = 4096: region-local offset 0 maps to device offset 4096.
        let mut flash = MockFlash::new(8192);
        flash.erase(0, 8192).expect("erase");
        let mut region = RegionStorage::<MockFlash, 4096, 4096>::new(flash);
        region.write(0, &[9, 9, 9, 9]).expect("write");
        let inner = region.into_inner();
        assert_eq!(&inner.bytes()[4096..4100], &[9, 9, 9, 9]);
        assert_eq!(&inner.bytes()[0..4], &[0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn ota_writer_sequential_chunks_verify_crc() {
        let image: Vec<u8> = (0..100u8).collect();
        let mut writer = OtaWriter::new(MockFlash::new(8192));

        for (offset, chunk) in image.chunks(28).scan(0u32, |offset, c| {
            let at = *offset;
            *offset += c.len() as u32;
            Some((at, c))
        }) {
            writer.write(offset, chunk).expect("chunk");
        }

        assert!(writer.verify(reference_crc(&image)).expect("verify"));

        let flash = writer.into_inner();
        assert_eq!(&flash.bytes()[4096..4096 + image.len()], &image[..]);
        assert!(flash.bytes()[4096 + image.len()..].iter().all(|&b| b == 0xFF));
    }

    #[test]
    fn ota_writer_unaligned_chunks() {
        // Chunks that land mid-word force head-word preservation.
        let image: Vec<u8> = (0..40u8).collect();
        let mut writer = OtaWriter::new(MockFlash::new(8192));

        for (offset, chunk) in image.chunks(3).scan(0u32, |offset, c| {
            let at = *offset;
            *offset += c.len() as u32;
            Some((at, c))
        }) {
            writer.write(offset, chunk).expect("chunk");
        }

        assert!(writer.verify(reference_crc(&image)).expect("verify"));
        let flash = writer.into_inner();
        assert_eq!(&flash.bytes()[4096..4096 + image.len()], &image[..]);
    }

    #[test]
    fn ota_writer_multiple_sessions_reset_crc() {
        // A write at offset 0 starts a new session: the running CRC is reset
        // and only the second image's bytes count toward verification.
        let mut writer = OtaWriter::new(MockFlash::new(8192));
        writer.write(0, b"first").expect("session 1");
        writer.write(5, b" image").expect("session 1");

        writer.write(0, b"second").expect("session 2");
        assert!(writer.verify(reference_crc(b"second")).expect("verify"));
    }

    #[test]
    fn ota_writer_rejects_out_of_region() {
        let mut writer = OtaWriter::new(MockFlash::new(8192));
        assert!(writer.write(4000, &[0u8; 100]).is_err());
        assert!(writer.write(4090, &[0u8; 10]).is_err());
    }

    #[test]
    fn ota_writer_verify_wrong_crc_fails() {
        let mut writer = OtaWriter::new(MockFlash::new(8192));
        writer.write(0, b"payload").expect("write");
        assert!(!writer.verify(0xDEADBEEF).expect("verify"));
        assert!(writer.verify(reference_crc(b"payload")).expect("verify"));
    }

    #[test]
    fn flash_writer_as_bytes_is_empty() {
        let mut writer = OtaWriter::new(MockFlash::new(8192));
        writer.write(0, b"data").expect("write");
        assert!(writer.as_bytes().is_empty());
    }
}

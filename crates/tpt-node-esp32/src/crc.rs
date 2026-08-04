//! Incremental CRC-32 (ISO/HDLC) for streamed image verification.
//!
//! `tpt-e-link` frames are protected with the standard CRC-32/ISO-HDLC
//! algorithm (`poly 0xEDB88320`, `init 0xFFFFFFFF`, reflected, `xorout
//! 0xFFFFFFFF`). The internal OTA handler in `tpt-node-core` expects a
//! `FlashWriter` that reports the accumulated CRC of everything written. This
//! module provides that digest as a small, dependency-free, `no_std`-friendly
//! state machine so the ESP32 flash writer can verify an image *while*
//! streaming it to flash instead of buffering the whole image in RAM.

use core::fmt;

const POLY: u32 = 0xEDB8_8320;
const INIT: u32 = 0xFFFF_FFFF;
const XOROUT: u32 = 0xFFFF_FFFF;

/// Incremental CRC-32/ISO-HDLC digest.
///
/// ```text
/// let mut crc = IsoHdlcCrc32::new();
/// crc.update(chunk1);
/// crc.update(chunk2);
/// assert_eq!(crc.finalize(), expected_crc);
/// ```
#[derive(Clone, Copy)]
pub struct IsoHdlcCrc32 {
    table: [u32; 256],
    state: u32,
}

impl fmt::Debug for IsoHdlcCrc32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IsoHdlcCrc32")
            .field("state", &self.state)
            .field("running", &self.finalize())
            .finish()
    }
}

impl Default for IsoHdlcCrc32 {
    fn default() -> Self {
        Self::new()
    }
}

impl IsoHdlcCrc32 {
    /// Creates a fresh digest initialized per the ISO/HDLC algorithm.
    pub const fn new() -> Self {
        let mut table = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut c = i as u32;
            let mut j = 0;
            while j < 8 {
                c = if c & 1 != 0 { POLY ^ (c >> 1) } else { c >> 1 };
                j += 1;
            }
            table[i] = c;
            i += 1;
        }
        Self { table, state: INIT }
    }

    /// Feeds bytes into the digest.
    pub fn update(&mut self, data: &[u8]) {
        for &b in data {
            let idx = ((self.state ^ b as u32) & 0xFF) as usize;
            self.state = self.table[idx] ^ (self.state >> 8);
        }
    }

    /// Returns the completed checksum (with `xorout` applied).
    pub fn finalize(&self) -> u32 {
        self.state ^ XOROUT
    }

    /// Re-initializes the digest for reuse.
    pub fn reset(&mut self) {
        self.state = INIT;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHECK_VECTOR: u32 = 0xCBF4_3926; // CRC-32/ISO-HDLC of "123456789"

    #[test]
    fn known_vector() {
        let mut crc = IsoHdlcCrc32::new();
        crc.update(b"123456789");
        assert_eq!(crc.finalize(), CHECK_VECTOR);
    }

    #[test]
    fn empty_input() {
        let crc = IsoHdlcCrc32::new();
        assert_eq!(crc.finalize(), 0);
    }

    #[test]
    fn incremental_matches_one_shot() {
        let data = b"The quick brown fox jumps over the lazy dog";
        let mut split = IsoHdlcCrc32::new();
        for chunk in data.chunks(7) {
            split.update(chunk);
        }
        let mut one_shot = IsoHdlcCrc32::new();
        one_shot.update(data);
        assert_eq!(split.finalize(), one_shot.finalize());
    }

    #[test]
    fn reset_reproduces() {
        let mut crc = IsoHdlcCrc32::new();
        crc.update(b"abc");
        crc.reset();
        crc.update(b"123456789");
        assert_eq!(crc.finalize(), CHECK_VECTOR);
    }

    #[test]
    fn matches_crc_crate_algorithm() {
        let mut manual = IsoHdlcCrc32::new();
        manual.update(b"some telemetry payload bytes");
        let reference = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC).checksum(b"some telemetry payload bytes");
        assert_eq!(manual.finalize(), reference);
    }
}

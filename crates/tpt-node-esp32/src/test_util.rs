//! Test-only helpers shared by the crate's unit tests.
//!
//! `MockFlash` is an in-memory NOR-flash emulator with real erase/program
//! semantics (writes may only clear bits; erased cells read as `0xFF`), so the
//! region, OTA and LittleFS code paths are exercised against realistic flash
//! behavior on the host.

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

use embedded_storage::nor_flash::{
    ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash,
};

/// In-memory NOR flash.
pub(crate) struct MockFlash {
    data: Vec<u8>,
}

impl MockFlash {
    /// Creates a flash device `size` bytes large, all cells erased.
    pub(crate) fn new(size: usize) -> Self {
        let mut data = Vec::with_capacity(size);
        data.resize(size, 0xFF);
        Self { data }
    }

    /// Raw cell contents (for assertions).
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.data
    }
}

/// Errors reported by [`MockFlash`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MockError {
    /// Read/write/erase beyond the device capacity.
    OutOfBounds,
    /// A program attempted to set a bit already programmed to 0.
    WriteConflict,
}

impl NorFlashError for MockError {
    fn kind(&self) -> NorFlashErrorKind {
        match self {
            MockError::OutOfBounds => NorFlashErrorKind::OutOfBounds,
            MockError::WriteConflict => NorFlashErrorKind::Other,
        }
    }
}

impl ErrorType for MockFlash {
    type Error = MockError;
}

impl ReadNorFlash for MockFlash {
    const READ_SIZE: usize = 4;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), MockError> {
        let start = offset as usize;
        let end = start + bytes.len();
        if end > self.data.len() {
            return Err(MockError::OutOfBounds);
        }
        bytes.copy_from_slice(&self.data[start..end]);
        Ok(())
    }

    fn capacity(&self) -> usize {
        self.data.len()
    }
}

impl NorFlash for MockFlash {
    const WRITE_SIZE: usize = 4;
    const ERASE_SIZE: usize = 4096;

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), MockError> {
        let start = offset as usize;
        let end = start + bytes.len();
        if end > self.data.len() {
            return Err(MockError::OutOfBounds);
        }
        for (i, &byte) in bytes.iter().enumerate() {
            let merged = self.data[start + i] & byte;
            if merged != byte {
                return Err(MockError::WriteConflict);
            }
            self.data[start + i] = merged;
        }
        Ok(())
    }

    fn erase(&mut self, from: u32, to: u32) -> Result<(), MockError> {
        for byte in &mut self.data[from as usize..to as usize] {
            *byte = 0xFF;
        }
        Ok(())
    }
}

//! Access to a user-supplied Super Mario Advance 4 ROM.
//!
//! Nothing from the ROM is bundled with this project; every asset is read at runtime
//! from the cartridge image the user dumped.

pub mod blocks;
pub mod decompress;
pub mod gfx;
pub mod level;
mod header;

pub use header::{Header, Version};

use std::path::Path;

/// Base address the cartridge is mapped at on the GBA bus.
pub const ROM_BASE: u32 = 0x0800_0000;

#[derive(Debug, thiserror::Error)]
pub enum RomError {
    #[error("could not read ROM file: {0}")]
    Io(#[from] std::io::Error),
    #[error("file is too small to be a GBA ROM ({0} bytes)")]
    TooSmall(usize),
    #[error("unsupported game code {0:?}; expected an SMA4 ROM (AX4E/AX4P/AX4J)")]
    WrongGame(String),
    #[error("header checksum mismatch (stored {stored:#04x}, computed {computed:#04x})")]
    BadChecksum { stored: u8, computed: u8 },
    #[error("address {0:#010x} is outside the ROM")]
    OutOfBounds(u32),
    #[error("bad compressed data at {addr:#010x}: {reason}")]
    Decompress { addr: u32, reason: &'static str },
}

pub type Result<T> = std::result::Result<T, RomError>;

pub struct Rom {
    data: Vec<u8>,
    header: Header,
}

impl Rom {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_bytes(std::fs::read(path)?)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Self> {
        let header = Header::parse(&data)?;
        Ok(Self { data, header })
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    fn offset(&self, addr: u32, len: usize) -> Result<usize> {
        let off = addr.checked_sub(ROM_BASE).ok_or(RomError::OutOfBounds(addr))? as usize;
        if off + len > self.data.len() {
            return Err(RomError::OutOfBounds(addr));
        }
        Ok(off)
    }

    /// Slice of ROM starting at a GBA bus address (0x08xxxxxx).
    pub fn slice(&self, addr: u32, len: usize) -> Result<&[u8]> {
        let off = self.offset(addr, len)?;
        Ok(&self.data[off..off + len])
    }

    /// Everything from `addr` to the end of the ROM.
    pub fn tail(&self, addr: u32) -> Result<&[u8]> {
        let off = self.offset(addr, 0)?;
        Ok(&self.data[off..])
    }

    pub fn u8(&self, addr: u32) -> Result<u8> {
        Ok(self.slice(addr, 1)?[0])
    }

    pub fn u16(&self, addr: u32) -> Result<u16> {
        let b = self.slice(addr, 2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn u32(&self, addr: u32) -> Result<u32> {
        let b = self.slice(addr, 4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Reads a pointer table entry and checks it points into the ROM.
    pub fn ptr(&self, addr: u32) -> Result<u32> {
        let p = self.u32(addr)?;
        self.offset(p, 0)?;
        Ok(p)
    }

    /// Decompresses BIOS-format data (LZ77/Huffman/RLE) stored at `addr`.
    pub fn decompress(&self, addr: u32) -> Result<Vec<u8>> {
        decompress::decompress(self.tail(addr)?)
            .map_err(|reason| RomError::Decompress { addr, reason })
    }
}

/// An LZ77 stream found by [`Rom::scan_lz77`].
#[derive(Debug, Clone, Copy)]
pub struct Found {
    pub addr: u32,
    pub packed_len: usize,
    pub unpacked_len: usize,
}

impl Rom {
    /// Heuristically finds LZ77 streams whose output is at least `min_len` bytes and a
    /// multiple of `align`. Used to locate graphics before their pointer tables are known.
    pub fn scan_lz77(&self, min_len: usize, align: usize) -> Vec<Found> {
        let mut found = Vec::new();
        let mut off = 0;
        while off + 4 <= self.data.len() {
            if self.data[off] == decompress::TYPE_LZ77
                && let Ok((out, used)) = decompress::lz77_with_len(&self.data[off..])
                && out.len() >= min_len
                && out.len() % align == 0
            {
                found.push(Found {
                    addr: ROM_BASE + off as u32,
                    packed_len: used,
                    unpacked_len: out.len(),
                });
                off += used.next_multiple_of(4);
                continue;
            }
            off += 4;
        }
        found
    }
}

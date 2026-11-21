//! Sound effects rendered by the ROM's own sound driver (`sma4-extract/src/sound.rs`).
//!
//! The driver mixes 8-bit PCM for Direct Sound A (left) and B (right) at timer 0 reload
//! 0xF9C4 (1596 cycles a sample, 10512 Hz, 176 samples a frame). A file holds the magic, the
//! driver slot the sound plays in (0x08308564[id] bits 16-23: a new sound in a slot stops
//! the one playing there) and the interleaved left / right samples (i8).

use std::io;
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"SMA4SE01";
/// 16777216 / 1596.
pub const SOUND_RATE: u32 = 10512;

/// The effects the port plays (`sma4_core::Game::sounds`): thunder, a cannonball stomped,
/// a ball meeting a shell (0x7F + chain 0-7), a cannon's shot.
pub const SOUND_IDS: &[u8] = &[0x18, 0x7E, 0x7F, 0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x8A];

pub fn sound_path(dir: &Path, id: u8) -> PathBuf {
    dir.join("sounds").join(format!("se_{id:02x}.bin"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundAsset {
    pub slot: u8,
    /// Left, right, left, right ...
    pub samples: Vec<i8>,
}

impl SoundAsset {
    pub fn write(&self, dir: &Path, id: u8) -> io::Result<()> {
        let path = sound_path(dir, id);
        std::fs::create_dir_all(path.parent().unwrap())?;
        let mut out = MAGIC.to_vec();
        out.push(self.slot);
        out.extend(self.samples.iter().map(|&s| s as u8));
        std::fs::write(path, out)
    }

    pub fn read(dir: &Path, id: u8) -> io::Result<Self> {
        let d = std::fs::read(sound_path(dir, id))?;
        if d.len() < 9 || &d[..8] != MAGIC {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("sound {id:#x}: bad file (re-run sma4-extract)")));
        }
        Ok(Self { slot: d[8], samples: d[9..].iter().map(|&b| b as i8).collect() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_round_trip() {
        let dir = std::env::temp_dir().join("sma4_sound_test");
        let s = SoundAsset { slot: 8, samples: vec![0, -1, 127, -128] };
        s.write(&dir, 0x8A).unwrap();
        assert_eq!(SoundAsset::read(&dir, 0x8A).unwrap(), s);
    }
}

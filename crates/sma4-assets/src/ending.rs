//! The end of a world after its castle (`ending/ending.bin`): the Princess's letter and
//! the "WORLD n CLEAR!" save screen, kept as the GBA's video memory the ROM's own loaders
//! leave (the game draws them the way the GBA would), and the ROM tables their logic
//! reads. See docs/re-notes.md "After the castle".

use std::path::{Path, PathBuf};

use super::{AssetError, Cursor, Result, read, write};

pub const ENDING_DIR: &str = "ending";
const ENDING_MAGIC: &[u8; 8] = b"SMA4EN01";

/// ROM ranges (start, end) copied into `EndingAsset::rom`: the restored throne room's
/// tables (box rows and their entries, the message by form, the King's and Toad's
/// sprites, the wand, Mario's poses, 0x0813F5E2..0x0813F9EF), the letter's sprites and
/// items (0x0813F8A8..), the save screen's tables and window lists (0x0814A2C8..0x0814B3D8,
/// 0x082A2400..0x082A2B00).
pub const ENDING_ROM: &[(u32, u32)] = &[(0x0813_F5E0, 0x0813_F9F0), (0x0814_A2C0, 0x0814_B3D8), (0x082A_2400, 0x082A_2B00)];

/// VRAM bytes kept per screen (BG chars and maps, OBJ tiles).
pub const ENDING_VRAM: usize = 0x1_8000;

/// One screen's video memory: VRAM and the palette buffer (0x02000000, BG then OBJ) as
/// the loaders leave them.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct VideoSnapshot {
    pub vram: Vec<u8>,
    pub palette: Vec<u16>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EndingAsset {
    /// The letter (0x08007BD0 and 0x0802F214 after the throne room's load).
    pub letter: VideoSnapshot,
    /// "WORLD n CLEAR!" (0x08039C30), per world.
    pub clear: Vec<VideoSnapshot>,
    /// "Saving..." OBJ tiles (LZ77 0x0814B2A4[language]), unpacked at 0x06017000 when
    /// Yes is chosen.
    pub saving: Vec<u8>,
    /// `ENDING_ROM`.
    pub rom: Vec<(u32, Vec<u8>)>,
}

impl EndingAsset {
    pub fn path(dir: &Path) -> PathBuf {
        dir.join(ENDING_DIR).join("ending.bin")
    }

    /// The ROM byte at `addr`; 0 outside `ENDING_ROM`.
    pub fn rom_u8(&self, addr: u32) -> u8 {
        self.rom
            .iter()
            .find(|(a, d)| (*a..*a + d.len() as u32).contains(&addr))
            .map_or(0, |(a, d)| d[(addr - a) as usize])
    }

    pub fn rom_u16(&self, addr: u32) -> u16 {
        u16::from_le_bytes([self.rom_u8(addr), self.rom_u8(addr + 1)])
    }

    pub fn rom_u32(&self, addr: u32) -> u32 {
        u32::from_le_bytes([self.rom_u8(addr), self.rom_u8(addr + 1), self.rom_u8(addr + 2), self.rom_u8(addr + 3)])
    }

    pub fn write(&self, dir: &Path) -> Result<()> {
        let mut out = ENDING_MAGIC.to_vec();
        let blob = |out: &mut Vec<u8>, d: &[u8]| {
            out.extend_from_slice(&(d.len() as u32).to_le_bytes());
            out.extend_from_slice(d);
        };
        let snap = |out: &mut Vec<u8>, s: &VideoSnapshot| {
            blob(out, &s.vram);
            let pal: Vec<u8> = s.palette.iter().flat_map(|c| c.to_le_bytes()).collect();
            blob(out, &pal);
        };
        snap(&mut out, &self.letter);
        out.extend_from_slice(&(self.clear.len() as u16).to_le_bytes());
        for s in &self.clear {
            snap(&mut out, s);
        }
        blob(&mut out, &self.saving);
        out.extend_from_slice(&(self.rom.len() as u16).to_le_bytes());
        for (a, d) in &self.rom {
            out.extend_from_slice(&a.to_le_bytes());
            blob(&mut out, d);
        }
        write(&Self::path(dir), &out)
    }

    pub fn read(dir: &Path) -> Result<Self> {
        let path = Self::path(dir);
        let d = read(&path)?;
        let bad = || AssetError::BadLevel(path.clone());
        if d.get(..8) != Some(ENDING_MAGIC) {
            return Err(bad());
        }
        let mut c = Cursor(&d[8..]);
        (|| {
            let blob = |c: &mut Cursor| {
                let n = u32::from_le_bytes(c.bytes(4)?.try_into().ok()?) as usize;
                Some(c.bytes(n)?.to_vec())
            };
            let snap = |c: &mut Cursor| {
                let vram = blob(c)?;
                let pal = blob(c)?;
                Some(VideoSnapshot { vram, palette: pal.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect() })
            };
            let mut a = EndingAsset { letter: snap(&mut c)?, ..Default::default() };
            for _ in 0..c.u16()? {
                a.clear.push(snap(&mut c)?);
            }
            a.saving = blob(&mut c)?;
            for _ in 0..c.u16()? {
                let addr = u32::from_le_bytes(c.bytes(4)?.try_into().ok()?);
                a.rom.push((addr, blob(&mut c)?));
            }
            Some(a)
        })()
        .ok_or_else(bad)
    }
}

/// The restored throne room's message box (0x0813F680: 10 rows of 0x1A BG3 entries),
/// empty, and with message `id` drawn whole.
pub fn ending_box_path(dir: &Path) -> PathBuf {
    dir.join(ENDING_DIR).join("box.png")
}
pub fn ending_text_path(dir: &Path, id: u8) -> PathBuf {
    dir.join(ENDING_DIR).join(format!("text_{id:02x}.png"))
}
pub const ENDING_BOX_H: u32 = 10;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ending_round_trip() {
        let dir = std::env::temp_dir().join(format!("sma4-ending-{}", std::process::id()));
        let snap = |v: u8| VideoSnapshot { vram: vec![v; 16], palette: vec![v as u16 * 3; 4] };
        let a = EndingAsset { letter: snap(1), clear: vec![snap(2), snap(3)], saving: vec![9; 5], rom: vec![(0x0813_F5E0, vec![1, 2])] };
        a.write(&dir).unwrap();
        assert_eq!(EndingAsset::read(&dir).unwrap(), a);
        assert_eq!(a.rom_u16(0x0813_F5E0), 0x0201);
        std::fs::remove_dir_all(&dir).ok();
    }
}

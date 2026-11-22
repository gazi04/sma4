//! The King's castle and the airship (`castle/`): the ROM tables their logic reads, the
//! messages of the throne room, and snapshots of OBJ VRAM for the objects drawn as
//! hardware sprites (`sma4_core::obj::ObjSprite`). See docs/re-notes.md "King's castle".

use std::path::{Path, PathBuf};

use super::{AssetError, Cursor, Result, read, write};

pub const CASTLE_DIR: &str = "castle";
const CASTLE_MAGIC: &[u8; 8] = b"SMA4CA01";
const BANK_MAGIC: &[u8; 8] = b"SMA4OB01";

/// ROM ranges (start, end) copied into `CastleAsset::rom`: the throne room's tables
/// (0x08171E04..: phase handlers, box rows, Toad's tiles, the King's offsets, animation
/// timers and tiles, attribute bytes 0x08172024, the extra piece's offsets 0x0817207C /
/// 0x08172092).
pub const CASTLE_ROM: &[(u32, u32)] = &[(0x0817_1E04, 0x0817_20C0)];

/// Throne-room messages (0x080AB0CC ids in the language's text table 0x08244318): the
/// first visit's "Oh, it's terrible!", the visit after the airship got away ("Hurry!
/// Hurry!") and the King changed back ("Oh, splendid!").
pub const THRONE_MESSAGE: u8 = 5;
pub const HURRY_MESSAGE: u8 = 6;
pub const RESTORED_MESSAGE: u8 = 7;
pub const CASTLE_MESSAGES: [u8; 3] = [THRONE_MESSAGE, HURRY_MESSAGE, RESTORED_MESSAGE];

/// The message box: 0x1A x 8 BG3 cells (rows 0x08171EC0[r]), top-left at BG3 cell (2, 0)
/// (map byte 4, 0x08093CE4); the text starts at cell (4, 1) (map entry 0x24).
pub const CASTLE_BOX_W: u32 = 0x1A;
pub const CASTLE_BOX_H: u32 = 8;
pub const CASTLE_BOX_AT: (u32, u32) = (2, 0);
/// The text's first cell, relative to the box.
pub const CASTLE_TEXT_AT: (u32, u32) = (2, 1);

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CastleAsset {
    /// ROM bytes the logic reads: `CASTLE_ROM`.
    pub rom: Vec<(u32, Vec<u8>)>,
    /// Message bytes by id (BG3 tile ids, 0xF0 next row, 0xFE blank cell, 0xFF end).
    pub messages: Vec<(u8, Vec<u8>)>,
}

impl CastleAsset {
    pub fn path(dir: &Path) -> PathBuf {
        dir.join(CASTLE_DIR).join("castle.bin")
    }

    /// The ROM byte at `addr`; 0 outside `CASTLE_ROM`.
    pub fn rom_u8(&self, addr: u32) -> u8 {
        self.rom
            .iter()
            .find(|(a, d)| (*a..*a + d.len() as u32).contains(&addr))
            .map_or(0, |(a, d)| d[(addr - a) as usize])
    }

    pub fn rom_u16(&self, addr: u32) -> u16 {
        u16::from_le_bytes([self.rom_u8(addr), self.rom_u8(addr + 1)])
    }

    pub fn message(&self, id: u8) -> &[u8] {
        self.messages.iter().find(|(i, _)| *i == id).map_or(&[0xFF], |(_, m)| m.as_slice())
    }

    pub fn write(&self, dir: &Path) -> Result<()> {
        let mut out = CASTLE_MAGIC.to_vec();
        let blob = |out: &mut Vec<u8>, d: &[u8]| {
            out.extend_from_slice(&(d.len() as u32).to_le_bytes());
            out.extend_from_slice(d);
        };
        out.extend_from_slice(&(self.rom.len() as u16).to_le_bytes());
        for (a, d) in &self.rom {
            out.extend_from_slice(&a.to_le_bytes());
            blob(&mut out, d);
        }
        out.extend_from_slice(&(self.messages.len() as u16).to_le_bytes());
        for (id, d) in &self.messages {
            out.push(*id);
            blob(&mut out, d);
        }
        write(&Self::path(dir), &out)
    }

    pub fn read(dir: &Path) -> Result<Self> {
        let path = Self::path(dir);
        let d = read(&path)?;
        let bad = || AssetError::BadLevel(path.clone());
        if d.get(..8) != Some(CASTLE_MAGIC) {
            return Err(bad());
        }
        let mut c = Cursor(&d[8..]);
        (|| {
            let blob = |c: &mut Cursor| {
                let n = u32::from_le_bytes(c.bytes(4)?.try_into().ok()?) as usize;
                Some(c.bytes(n)?.to_vec())
            };
            let mut a = CastleAsset::default();
            for _ in 0..c.u16()? {
                let addr = u32::from_le_bytes(c.bytes(4)?.try_into().ok()?);
                a.rom.push((addr, blob(&mut c)?));
            }
            for _ in 0..c.u16()? {
                let id = c.u8()?;
                a.messages.push((id, blob(&mut c)?));
            }
            Some(a)
        })()
        .ok_or_else(bad)
    }
}

/// Larry's palette 12 while a fireball's hit flashes him (16 colours, 0x0821F300 through
/// the colour curve); the host gives it OBJ palette number 16 (`sma4_core::larry`).
pub fn larry_flash_path(dir: &Path) -> PathBuf {
    dir.join(CASTLE_DIR).join("larry_flash.bin")
}

/// The castle's message box with no text, and with message `id` drawn whole (the game
/// reveals it cell by cell).
pub fn castle_box_path(dir: &Path) -> PathBuf {
    dir.join(CASTLE_DIR).join("box.png")
}
pub fn castle_text_path(dir: &Path, id: u8) -> PathBuf {
    dir.join(CASTLE_DIR).join(format!("text_{id:02x}.png"))
}

/// OBJ VRAM (0x06010000, 0x8000 bytes, 1D mapping off: 32 tiles a row) and the OBJ palette
/// buffer (0x02000200, 0x200 bytes) as a room leaves them, for objects drawn as hardware
/// sprites.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ObjBank {
    pub vram: Vec<u8>,
    pub palette: Vec<u8>,
}

pub const OBJ_VRAM_BYTES: usize = 0x8000;
pub const OBJ_PALETTE_BYTES: usize = 0x200;

impl ObjBank {
    /// The bank of the room `level` (hex level id).
    pub fn path(dir: &Path, level: u16) -> PathBuf {
        dir.join(CASTLE_DIR).join(format!("obj_{level:03x}.bin"))
    }

    pub fn write(&self, dir: &Path, level: u16) -> Result<()> {
        let mut out = BANK_MAGIC.to_vec();
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.palette);
        write(&Self::path(dir, level), &out)
    }

    pub fn read(dir: &Path, level: u16) -> Result<Self> {
        let path = Self::path(dir, level);
        let d = read(&path)?;
        if d.get(..8) != Some(BANK_MAGIC) || d.len() != 8 + OBJ_VRAM_BYTES + OBJ_PALETTE_BYTES {
            return Err(AssetError::BadLevel(path));
        }
        Ok(Self { vram: d[8..8 + OBJ_VRAM_BYTES].to_vec(), palette: d[8 + OBJ_VRAM_BYTES..].to_vec() })
    }

    /// RGBA pixels (alpha 0 for colour 0) of an OBJ: `w` x `h` px from `tile` (2D mapping,
    /// 32 tiles a row) in palette `pal`, flipped as asked.
    pub fn render(&self, tile: u16, (w, h): (u32, u32), pal: u8, hflip: bool, vflip: bool) -> Vec<u8> {
        let mut px = vec![0u8; (w * h * 4) as usize];
        for sy in 0..h {
            for sx in 0..w {
                let t = (tile as u32 + (sy / 8) * 32 + sx / 8) as usize & 0x3FF;
                let b = self.vram[t * 32 + ((sy % 8) * 4 + (sx % 8) / 2) as usize];
                let ci = if sx & 1 == 1 { b >> 4 } else { b & 0xF };
                if ci == 0 {
                    continue;
                }
                // Palettes past 15 are extra ones the host appended (Larry's flash).
                let o = (pal as usize * 16 + ci as usize) * 2;
                let Some(&[lo, hi]) = self.palette.get(o..o + 2) else { continue };
                let c = u16::from_le_bytes([lo, hi]);
                let ch = |s: u16| {
                    let v = ((c >> s) & 0x1F) as u8;
                    (v << 3) | (v >> 2)
                };
                let dx = if hflip { w - 1 - sx } else { sx };
                let dy = if vflip { h - 1 - sy } else { sy };
                let i = ((dy * w + dx) * 4) as usize;
                px[i..i + 4].copy_from_slice(&[ch(0), ch(5), ch(10), 255]);
            }
        }
        px
    }
}

/// OBJ sizes by shape (square, wide, tall) and size.
pub const OBJ_SIZES: [[(u32, u32); 4]; 3] = [
    [(8, 8), (16, 16), (32, 32), (64, 64)],
    [(16, 8), (32, 8), (32, 16), (64, 32)],
    [(8, 16), (8, 32), (16, 32), (32, 64)],
];

/// The airship's sky: BG2 (the dark clouds, `front` false) and BG3 (the white clouds in
/// front), 512x256 each. BG2 is in the level's own colours; `sky_flash_path` has it in
/// each colour step of the lightning. BG3 is alpha-blended over everything (BLDCNT 0x1768,
/// BLDALPHA 0x090A: 10/16 of it plus 9/16 of what is behind), so its picture holds an
/// alpha and colours that, drawn with ordinary alpha blending, give its brightest colour
/// exactly over black and let a little less (1 - alpha, about 1/2) of the rest through.
pub fn sky_path(dir: &Path, front: bool) -> PathBuf {
    dir.join(CASTLE_DIR).join(if front { "sky_front.png" } else { "sky_back.png" })
}
/// BG2 in lightning step `step` (0-5, `sma4_core::sky`); step 5 is the dark sky between
/// flashes.
pub fn sky_flash_path(dir: &Path, step: u8) -> PathBuf {
    dir.join(CASTLE_DIR).join(format!("sky_back_{step}.png"))
}
pub const SKY_FLASH_STEPS: u8 = 6;
/// The intro's clouds in front (level 0x202, scene mode 8: BG3, 512x256), blended as the
/// deck's front clouds; at x camera / 8 (0x03003BBE set), y 0x17 (VBlank 0x0800092C).
pub fn intro_clouds_path(dir: &Path) -> PathBuf {
    dir.join(CASTLE_DIR).join("intro_clouds.png")
}
pub const INTRO_CLOUDS_Y: i32 = 0x17;
/// BG2 at y 0x3F (VBlank 0x0800092C for scene 2: 0x03003DC6 + 0x3F; across, line by line,
/// `sma4_core::sky::Sky::back_hofs`); BG3 at y 0x17, across by `Sky::front_x`.
pub const SKY_BACK_Y: i32 = 0x3F;
pub const SKY_FRONT_Y: i32 = 0x17;

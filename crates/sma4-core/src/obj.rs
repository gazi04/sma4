//! Hardware sprites (OAM entries) for objects whose draw routines are ported as they are:
//! the core writes the entries the ROM would write, the host draws them from the room's
//! OBJ VRAM snapshot (`sma4_assets::ObjBank`).

/// One OAM entry, in GBA screen pixels (sign-resolved: the ROM's 8-bit y and 9-bit x wrap).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ObjSprite {
    pub x: i32,
    pub y: i32,
    pub tile: u16,
    /// 0 square, 1 wide, 2 tall (attr0 bits 14-15).
    pub shape: u8,
    /// attr1 bits 14-15.
    pub size: u8,
    pub pal: u8,
    pub hflip: bool,
    pub vflip: bool,
    /// 0 (front) to 3 (behind the level's tiles).
    pub prio: u8,
    /// Semi-transparent (attr0 OBJ mode 1).
    pub blend: bool,
}

impl ObjSprite {
    /// From the three attribute words as the ROM writes them.
    pub fn from_attrs(a0: u16, a1: u16, a2: u16) -> Option<Self> {
        // Disabled (rotation off, bit 9 set).
        if a0 & 0x0300 == 0x0200 {
            return None;
        }
        let y = (a0 & 0xFF) as i32;
        let x = (a1 & 0x1FF) as i32;
        Some(Self {
            x: if x >= 0x100 { x - 0x200 } else { x },
            // Entries low on the 256-line wrap are shown at the top.
            y: if y >= 0xC0 { y - 0x100 } else { y },
            tile: a2 & 0x3FF,
            shape: (a0 >> 14) as u8,
            size: (a1 >> 14) as u8,
            pal: (a2 >> 12) as u8,
            hflip: a1 & 0x1000 != 0,
            vflip: a1 & 0x2000 != 0,
            prio: ((a2 >> 10) & 3) as u8,
            blend: (a0 >> 10) & 3 == 1,
        })
    }

    /// Width and height in pixels.
    pub fn dims(&self) -> (u32, u32) {
        const SIZES: [[(u32, u32); 4]; 3] = [
            [(8, 8), (16, 16), (32, 32), (64, 64)],
            [(16, 8), (32, 8), (32, 16), (64, 32)],
            [(8, 16), (8, 32), (16, 32), (32, 64)],
        ];
        SIZES[self.shape.min(2) as usize][self.size as usize & 3]
    }
}

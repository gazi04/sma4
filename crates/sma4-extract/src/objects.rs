//! Object (sprite) graphics, rendered from OBJ VRAM and OBJ palette RAM as the level
//! loaders leave them in the emulated machine.

use sma4_assets::{
    PLAYER_FRAMES, PLAYER_H, PLAYER_W, Rgba, leaf_path, coin_spin_path, growth_item_path, koopa_path, player_path, sparkle_path,
    walker_path,
};
use sma4_rom::{Rom, gfx::parse_palette};

use crate::emu::Machine;
use crate::level::ExtractError;
use crate::oam::{self, Pose};

const OBJ_VRAM: u32 = 0x0601_0000;
/// OBJ palette mirror built by the palette loader (0x080085AC).
const OBJ_PAL_BUFFER: u32 = 0x0200_0200;
/// OBJ tiles use 2D mapping: 32 tiles per row.
const TILES_PER_ROW: u32 = 32;

/// Coin popping out of a block (draw routine 0x0808A7F8): frame f = (counter >> 2) & 3,
/// tile 0x0816B954[f], h-flip 0x0816B958[f] bit 0; frame 0 is a 16x16 sprite drawn 4 px
/// further left, the others are 8x16. All use OBJ palette 0.
const COIN_TILES: u32 = 0x0816_B954;
const COIN_FLIPS: u32 = 0x0816_B958;
const COIN_FRAMES: u32 = 4;

/// Coin-collect sparkle (effect type 3, draw 0x08090B0C): 8x8 OBJ tiles 0x68-0x6C, palette 1.
const SPARKLE_TILE: u32 = 0x68;
const SPARKLE_TILES: u32 = 5;
const SPARKLE_PALETTE: u32 = 1;

/// Basic walking enemy, object type 0x72. Its draw routine 0x08053664 -> 0x0804821C puts
/// two 8x16 sprites side by side: tiles from 0x0816B4A0[type] + frame * 4 (left, right),
/// OBJ palette 0x0816A770[type] & 7. In this level type (0x0816E598 bit 0x10, mirror mode
/// 0x080484C4) the right sprite is h-flipped. Frames 0/1 walk (frame = (counter >> 2) & 1);
/// the squished state 7 (0x08044504) draws frame 3.
const WALKER: u32 = 0x72;
const OBJ_FRAME_TILES: u32 = 0x0816_B4A0;
const OBJ_PALETTES: u32 = 0x0816_A770;
const WALKER_FRAMES: [u32; 3] = [0, 1, 3];

/// Player graphics are streamed into OBJ VRAM every frame. 0x080A6D80 picks the pose's
/// DMA chunk list (pose = player byte +0x38, form +0x37, character 0x03002D77, source
/// 0x081736AC[character * 8 + form]) and 0x080A3EB0 copies it into the 32x32 area at OBJ
/// tile 0. The player is one 32x32 sprite (0x080A6E0C) with OBJ palette 7; its art faces
/// left and fills the left 16 columns (h-flipped and moved 16 px left when facing right).
const PLAYER_PTR: u32 = 0x0300_7818;
const PLAYER_GFX_PTR: u32 = 0x0300_7820;
const PLAYER_SCRATCH: u32 = 0x0203_A000;
const PLAYER_GFX_SCRATCH: u32 = 0x0203_B000;
const CHARACTER: u32 = 0x0300_2D77;
const FN_PLAYER_POSE_GFX: u32 = 0x080A_6D80;
const FN_PLAYER_DMA: u32 = 0x080A_3EB0;
const PLAYER_PALETTE: u32 = 7;
/// Ground pose 0x0813DD77[form * 4 + step] (0x08097800); jump pose 0x0813DEE8[form];
/// P-speed run 0x0813DEA8[step + 4 small / 0 big / 8 raccoon] (0x08097ACC); big falling
/// pose 0x06 (0x080983F8); raccoon air poses 0x0813DEB4 (0x080981C4: flying up, flying
/// down, jump, falling, by tail-wag step); tail whip 0x0813DE6E[step (+5 in the air)].
const WALK_POSES: u32 = 0x0813_DD77;
const JUMP_POSES: u32 = 0x0813_DEE8;
const RUN_POSES: u32 = 0x0813_DEA8;
const RACCOON_AIR_POSES: u32 = 0x0813_DEB4;
const WHIP_POSES: u32 = 0x0813_DE6E;
const BIG_FALL_POSE: u8 = 0x06;
const RACCOON: u8 = 3;
/// Death pose (0x080A8FC0 draws 0x4B with the form forced to small).
const DEATH_POSE: u8 = 0x4B;

struct Sheet<'a> {
    chars: &'a [u8],
    palette: Vec<sma4_rom::gfx::Bgr555>,
}

impl Sheet<'_> {
    /// Draws one 8x8 tile at (ox, oy) of an RGBA image `w` pixels wide.
    fn tile(&self, out: &mut [u8], w: u32, tile: u32, ox: u32, oy: u32, hflip: bool, pal: u32) {
        self.tile_flip(out, w, tile, ox, oy, hflip, false, pal);
    }

    #[allow(clippy::too_many_arguments)]
    fn tile_flip(&self, out: &mut [u8], w: u32, tile: u32, ox: u32, oy: u32, hflip: bool, vflip: bool, pal: u32) {
        let t = &self.chars[(tile * 32) as usize..][..32];
        for y in 0..8 {
            for x in 0..8 {
                let sx = if hflip { 7 - x } else { x };
                let sy = if vflip { 7 - y } else { y };
                let b = t[(sy * 4 + sx / 2) as usize];
                let ci = if sx & 1 == 1 { b >> 4 } else { b & 0xF };
                if ci == 0 {
                    continue;
                }
                let c = self.palette[(pal * 16 + ci as u32) as usize].to_rgba(255);
                let o = (((oy + y) * w + ox + x) * 4) as usize;
                out[o..o + 4].copy_from_slice(&c);
            }
        }
    }
}

/// Debug: the whole OBJ VRAM as a 32x32-tile sheet, once per OBJ palette.
pub fn dump_obj_vram(m: &Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    let chars = m.read(OBJ_VRAM, 0x8000)?;
    let sheet = Sheet { chars: &chars, palette: parse_palette(&m.read(OBJ_PAL_BUFFER, 0x200)?) };
    std::fs::write(out_dir.join("obj_pal.bin"), m.read(OBJ_PAL_BUFFER, 0x200)?).map_err(|e| ExtractError(e.to_string()))?;
    // BG VRAM too (64 KiB = 2048 tiles), with the BG palettes.
    let bg = m.read(0x0600_0000, 0x1_0000)?;
    let bg_sheet = Sheet { chars: &bg, palette: parse_palette(&m.read(0x0200_0000, 0x200)?) };
    std::fs::write(out_dir.join("bg_pal.bin"), m.read(0x0200_0000, 0x200)?).map_err(|e| ExtractError(e.to_string()))?;
    for pal in 0..16 {
        let (w, h) = (256, 512);
        let mut px = vec![0u8; (w * h * 4) as usize];
        for t in 0..2048 {
            bg_sheet.tile(&mut px, w, t, (t % 32) * 8, (t / 32) * 8, false, pal);
        }
        Rgba { width: w, height: h, pixels: px }
            .write_png(&out_dir.join(format!("bg_vram_pal{pal:x}.png")))
            .map_err(|e| ExtractError(e.to_string()))?;
    }
    let (w, h) = (256, 256);
    for pal in 0..16 {
        let mut px = vec![0u8; (w * h * 4) as usize];
        for t in 0..1024 {
            sheet.tile(&mut px, w, t, (t % 32) * 8, (t / 32) * 8, false, pal);
        }
        Rgba { width: w, height: h, pixels: px }
            .write_png(&out_dir.join(format!("obj_vram_pal{pal:x}.png")))
            .map_err(|e| ExtractError(e.to_string()))?;
    }
    Ok(())
}

const BG_VRAM: u32 = 0x0600_0000;
const BG_PAL_BUFFER: u32 = 0x0200_0000;

/// Status bar: template map (LZ77 0x0821F788, loaded by 0x08016434 for normal levels),
/// digit tiles 0x08131E6C, character icon 0x08131E64, and the tiles 0x280-0x2FF it uses
/// (BG char base 0) in palettes 0 and 15.
const HUD_TEMPLATE: u32 = 0x0821_F788;
const HUD_DIGITS: u32 = 0x0813_1E6C;
const HUD_ICON: u32 = 0x0813_1E64;

pub fn extract_hud(rom: &Rom, m: &Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    use sma4_assets::{HUD_COLS, HUD_ROWS, HUD_TILE_BASE, HUD_TILES, HudAsset};
    let map = rom.decompress(HUD_TEMPLATE)?;
    let template: Vec<u16> =
        map.chunks_exact(2).take(HUD_COLS * HUD_ROWS).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
    let digits = std::array::from_fn(|i| rom.u16(HUD_DIGITS + i as u32 * 2).unwrap_or(0xFF));
    let icon = [rom.u16(HUD_ICON)?, rom.u16(HUD_ICON + 2)?];
    HudAsset { template, digits, icon }.write(out_dir).map_err(|e| ExtractError(e.to_string()))?;

    let chars = m.read(BG_VRAM, 0x1_0000)?;
    let sheet = Sheet { chars: &chars, palette: parse_palette(&m.read(BG_PAL_BUFFER, 0x200)?) };
    let w = 8 * 2 * HUD_TILES as u32;
    let mut px = vec![0u8; (w * 8 * 4) as usize];
    for (half, pal) in [(0u32, 0u32), (1, 15)] {
        for i in 0..HUD_TILES as u32 {
            let cell = half * HUD_TILES as u32 + i;
            sheet.tile(&mut px, w, HUD_TILE_BASE as u32 + i, cell * 8, 0, false, pal);
        }
    }
    Rgba { width: w, height: 8, pixels: px }
        .write_png(&HudAsset::tiles_path(out_dir))
        .map_err(|e| ExtractError(e.to_string()))?;
    println!("  status bar");
    Ok(())
}

/// Score popups (0x08093750): two 8x8 OBJs per type t (1-12), tiles 0x08171DA4[t-1]
/// (0xFF = none) and 0x08171DBE[t-1], OBJ palette 0. Types 13-15 are drawn by 0x080938B4
/// instead: one 32x16 OBJ, tile 0x08171DD8[t-13] (1UP 0x54, 2UP, 3UP), palette 1.
const POPUP_LEFT: u32 = 0x0817_1DA4;
const POPUP_RIGHT: u32 = 0x0817_1DBE;
const LIFE_POPUP_TILES: u32 = 0x0817_1DD8;

pub fn extract_score_popups(rom: &Rom, m: &Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    use sma4_assets::{SCORE_POPUP_TYPES, score_popups_path};
    let chars = m.read(OBJ_VRAM, 0x8000)?;
    let sheet = Sheet { chars: &chars, palette: parse_palette(&m.read(OBJ_PAL_BUFFER, 0x200)?) };
    let w = 16 * SCORE_POPUP_TYPES as u32;
    let mut px = vec![0u8; (w * 8 * 4) as usize];
    for t in 0..SCORE_POPUP_TYPES as u32 {
        let pal = 0;
        let left = rom.u16(POPUP_LEFT + t * 2)?;
        if left != 0xFF {
            sheet.tile(&mut px, w, (left & 0x3FF) as u32, t * 16, 0, false, pal);
        }
        let right = rom.u16(POPUP_RIGHT + t * 2)?;
        sheet.tile(&mut px, w, (right & 0x3FF) as u32, t * 16 + 8, 0, false, pal);
    }
    Rgba { width: w, height: 8, pixels: px }
        .write_png(&score_popups_path(out_dir))
        .map_err(|e| ExtractError(e.to_string()))?;

    use sma4_assets::{LIFE_POPUP_TYPES, life_popups_path};
    let w = 32 * LIFE_POPUP_TYPES as u32;
    let mut px = vec![0u8; (w * 16 * 4) as usize];
    for t in 0..LIFE_POPUP_TYPES as u32 {
        let tile = (rom.u16(LIFE_POPUP_TILES + t * 2)? & 0x3FF) as u32;
        for ty in 0..2 {
            for tx in 0..4 {
                sheet.tile(&mut px, w, tile + ty * TILES_PER_ROW + tx, t * 32 + tx * 8, ty * 8, false, 1);
            }
        }
    }
    Rgba { width: w, height: 16, pixels: px }
        .write_png(&life_popups_path(out_dir))
        .map_err(|e| ExtractError(e.to_string()))?;
    println!("  score popups");
    Ok(())
}

/// Sparkle frames side by side in 8x8 cells, smallest first.
pub fn extract_sparkle(m: &Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    let chars = m.read(OBJ_VRAM, 0x8000)?;
    let sheet = Sheet { chars: &chars, palette: parse_palette(&m.read(OBJ_PAL_BUFFER, 0x200)?) };
    let w = 8 * SPARKLE_TILES;
    let mut px = vec![0u8; (w * 8 * 4) as usize];
    for i in 0..SPARKLE_TILES {
        sheet.tile(&mut px, w, SPARKLE_TILE + i, i * 8, 0, false, SPARKLE_PALETTE);
    }
    Rgba { width: w, height: 8, pixels: px }
        .write_png(&sparkle_path(out_dir))
        .map_err(|e| ExtractError(e.to_string()))?;
    println!("  sparkle: {SPARKLE_TILES} frames");
    Ok(())
}

/// Koopa Troopas (green 0x6C, red 0x6D share draw 0x080539E8: head 8x8 tile 0x120, body
/// two 8x16 from 0x0816B4A0, feet 16x8 tile 0x103/0x123), rendered by running the ROM's
/// draw routines (see `oam`):
/// * walk: 0x08053664 with frame +0x17 = 0/1
/// * shell (state 3): 0x08043060, frame 2; timer +0x34 below 0x50 shows the legs on ticks
///   where timer & 0x08169436[timer >> 4] != 0
/// * kicked (state 5): 0x08042E2C, spin step from global 0x03003CF6 >> 1
/// * Paratroopa 0x6E: wings 0x107/0x108 by (+0x3A + 2) & 4, walk frame (+0x3A >> 2) & 1.
const FN_DRAW_WALKER: u32 = 0x0805_3664;
const FN_DRAW_SHELL: u32 = 0x0804_3060;
const FN_DRAW_SPIN: u32 = 0x0804_2E2C;
const SPIN_COUNTER: u32 = 0x0300_3CF6;

pub fn extract_koopas(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    for kind in [0x6Cu16, 0x6D] {
        let walk = |f: u8| [(0x17, f)];
        let (w0, w1) = (walk(0), walk(1));
        let shell = [(0x17, 2), (0x34, 0xFF)];
        let waking = [(0x17, 2), (0x34, 0x02)];
        let spin: Vec<[(u32, u8); 1]> = (0..4).map(|k| [(SPIN_COUNTER, k * 2)]).collect();
        let mut poses = vec![
            Pose { kind, state: 2, draw: FN_DRAW_WALKER, bytes: &w0, globals: &[] },
            Pose { kind, state: 2, draw: FN_DRAW_WALKER, bytes: &w1, globals: &[] },
            Pose { kind, state: 3, draw: FN_DRAW_SHELL, bytes: &shell, globals: &[] },
            Pose { kind, state: 3, draw: FN_DRAW_SHELL, bytes: &waking, globals: &[] },
        ];
        for g in &spin {
            poses.push(Pose { kind, state: 5, draw: FN_DRAW_SPIN, bytes: &[], globals: g });
        }
        oam::render(m, rom.bytes(), &poses)?
            .write_png(&koopa_path(out_dir, kind))
            .map_err(|e| ExtractError(e.to_string()))?;
        println!("  koopa {kind:#x}: {} frames", poses.len());
    }
    let para: Vec<[(u32, u8); 2]> = [0u8, 2, 4, 6].iter().map(|&c| [(0x17, (c >> 2) & 1), (0x3A, c)]).collect();
    let poses: Vec<Pose> =
        para.iter().map(|b| Pose { kind: 0x6E, state: 2, draw: FN_DRAW_WALKER, bytes: b, globals: &[] }).collect();
    oam::render(m, rom.bytes(), &poses)?
        .write_png(&koopa_path(out_dir, 0x6E))
        .map_err(|e| ExtractError(e.to_string()))?;
    println!("  paratroopa 0x6e: {} frames", poses.len());
    Ok(())
}

/// Piranha Plants (green 0xA0, red 0xA2; main 0x08055C88, draw 0x0805564C): a 16x16 head
/// at y + 1 (tiles 0x0816B4A0[type], palette 0x0816A770[type]) over a 16x16 stem at y + 17
/// (palette 2). The draw routine picks frame (+0x3C >> 3) & 1, +2 while the plant is less
/// than 0x11 px above its home y (+0x35:+0x3A), which swaps the stem tile.
const FN_DRAW_PIRANHA: u32 = 0x0805_564C;
/// The object's y while rendering: `oam::AT` moved up by the difference between
/// `oam::ORIGIN` and `sma4_assets::PIRANHA_ORIGIN`, so the 33 px plant fits the cell; and
/// the home y bytes that put it all the way out (tall plant) or 5 px out.
const PIRANHA_Y: u8 = 64 - 16;
const PIRANHA_OUT: u8 = PIRANHA_Y + 0x21;
const PIRANHA_LOW: u8 = PIRANHA_Y + 5;

pub fn extract_piranhas(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    use sma4_assets::{PIRANHA_FRAMES, piranha_path};
    for kind in [0xA0u16, 0xA2] {
        debug_assert_eq!(oam::ORIGIN.1 - sma4_assets::PIRANHA_ORIGIN.1, 64 - PIRANHA_Y as i32);
        let bytes: Vec<[(u32, u8); 4]> = [(PIRANHA_OUT, 0), (PIRANHA_OUT, 8), (PIRANHA_LOW, 0), (PIRANHA_LOW, 8)]
            .iter()
            .map(|&(home, chomp)| [(0x09, PIRANHA_Y), (0x3A, home), (0x1C, 0x11), (0x3C, chomp)])
            .collect();
        let poses: Vec<Pose> =
            bytes.iter().map(|b| Pose { kind, state: 2, draw: FN_DRAW_PIRANHA, bytes: b, globals: &[] }).collect();
        debug_assert_eq!(poses.len(), PIRANHA_FRAMES as usize);
        oam::render(m, rom.bytes(), &poses)?
            .write_png(&piranha_path(out_dir, kind))
            .map_err(|e| ExtractError(e.to_string()))?;
        println!("  piranha {kind:#x}: {} frames", poses.len());
    }
    Ok(())
}

/// Puff of smoke, object state 8 (0x08044550, entered from state 6 by enemies whose
/// 0x0816A89D class is 7): timer +0x23 starts at 0x1F; two 8x16 OBJs at x and x + 8, tile
/// 0x08169484[timer >> 3], palette 1, the halves h-flipped by turns (0x03003CF6 >> 1).
const FN_PUFF: u32 = 0x0804_4550;
const PUFF_STATE: u8 = 8;

pub fn extract_puff(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    use sma4_assets::{PUFF_FRAMES, puff_path};
    let timers: Vec<[(u32, u8); 1]> = (0..PUFF_FRAMES as u8).map(|i| [(0x23, 0x1F - i * 8)]).collect();
    let poses: Vec<Pose> = timers
        .iter()
        .map(|b| Pose { kind: 0xA0, state: PUFF_STATE, draw: FN_PUFF, bytes: b, globals: &[(SPIN_COUNTER, 0)] })
        .collect();
    let cells = oam::render(m, rom.bytes(), &poses)?;
    // Cut each 16x16 out of its cell (sprites at the object's y, as 0x08044550 draws them).
    let (ox, oy) = (oam::ORIGIN.0 as u32, oam::ORIGIN.1 as u32);
    let w = 16 * PUFF_FRAMES;
    let mut px = vec![0u8; (w * 16 * 4) as usize];
    for f in 0..PUFF_FRAMES {
        for y in 0..16 {
            let src = ((((oy + y) * cells.width) + f * oam::CELL_W + ox) * 4) as usize;
            let dst = ((y * w + f * 16) * 4) as usize;
            px[dst..dst + 64].copy_from_slice(&cells.pixels[src..src + 64]);
        }
    }
    Rgba { width: w, height: 16, pixels: px }.write_png(&puff_path(out_dir)).map_err(|e| ExtractError(e.to_string()))?;
    println!("  puff: {PUFF_FRAMES} frames");
    Ok(())
}

/// Goal roulette card, object type 0x41 (init 0x0806826C, main 0x08068248). Its draw
/// 0x08067708 shows frame +0x17 (0-2 roulette, 3-5 the card flying up, 0x0816EFDC) with
/// palette 0x0816EF68[frame] >> 1; 0x080675D0 draws the 32x32 box around it (16 8x8 OBJs,
/// layout 0x0816EF58, tiles 0x0816EF38) at the original position +0x46/+0x4A - 8.
const GOAL: u16 = 0x41;
const FN_DRAW_GOAL: u32 = 0x0806_7708;
const FN_DRAW_GOAL_BOX: u32 = 0x0806_75D0;

pub fn extract_goal(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    use sma4_assets::{GOAL_FRAMES, goal_path};
    let frames: Vec<[(u32, u8); 1]> = (0..GOAL_FRAMES as u8 - 1).map(|f| [(0x17, f)]).collect();
    let mut poses: Vec<Pose> =
        frames.iter().map(|b| Pose { kind: GOAL, state: 2, draw: FN_DRAW_GOAL, bytes: b, globals: &[] }).collect();
    let at = oam::AT as u8;
    let box_bytes = [(0x3D, 0xFF), (0x46, at), (0x47, 0), (0x4A, at), (0x4B, 0)];
    poses.push(Pose { kind: GOAL, state: 2, draw: FN_DRAW_GOAL_BOX, bytes: &box_bytes, globals: &[] });
    oam::render(m, rom.bytes(), &poses)?.write_png(&goal_path(out_dir)).map_err(|e| ExtractError(e.to_string()))?;
    println!("  goal card: {} frames", poses.len());
    Ok(())
}

/// Course-clear texts (0x08068004 shows 0xAF at timer 0x30, 0xB0 at 2): language strings
/// 0x081431BC[lang * 3 + id - 0xAE] are VRAM write lists for 0x0802AAE0 ([u16 VRAM offset,
/// u16 count | 0x8000 down | 0x4000 repeat, entries...], 0xFFFF ends; entry bits 0-9 tile,
/// 10-12 palette, 14-15 flips), into the 32x32 map at VRAM 0xA000 over BG char base 0.
/// Glyphs: see `extract_course_clear`. The card that then appears in the frame
/// (0x080671AC: 2x2 BG tiles 0x2A7-0x2AC from 0x0816EE96 / 0x0816EEAE) is not extracted:
/// those tiles are not loaded in the extractor's machine; the game uses the card sprite.
const TEXTS: u32 = 0x0814_31BC;
/// Language 0x03003777: 0 English, 1 French (then German, Spanish, Italian).
const LANG: u32 = 0;
const MAP_FONTS: u32 = 0x0814_0CD8;
const MAP_FONT_OFFSET: usize = 0x3000;
const CLEAR_FONTS: u32 = 0x0816_EFA8;
const TEXT_MAP: u32 = 0xA000;

/// Applies one 0x0802AAE0 write list to a 32x32 map of (tile, palette, hflip, vflip).
fn apply_text(rom: &Rom, mut p: u32, map: &mut [Option<(u32, u32, bool, bool)>]) -> Result<(), ExtractError> {
    let decode = |v: u16| ((v & 0x3FF) as u32, ((v >> 10) & 7) as u32, v & 0x4000 != 0, v & 0x8000 != 0);
    while rom.u16(p)? != 0xFFFF {
        let cell = (rom.u16(p)? as u32 - TEXT_MAP) / 2;
        let n = rom.u16(p + 2)?;
        let (down, repeat, count) = (n & 0x8000 != 0, n & 0x4000 != 0, (n & 0x3FFF) as u32);
        for i in 0..count {
            let v = rom.u16(p + 4 + if repeat { 0 } else { i * 2 })?;
            let c = cell + if down { i * 32 } else { i };
            if let Some(slot) = map.get_mut(c as usize) {
                *slot = Some(decode(v));
            }
        }
        p += 4 + 2 * if repeat { 1 } else { count };
    }
    Ok(())
}

pub fn extract_course_clear(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    use sma4_assets::{
        CARD_GOT_AT, CARD_GOT_TILES, COURSE_CLEAR_AT, COURSE_CLEAR_TILES, card_got_path, course_clear_path,
    };
    // The level's status-bar font fills tiles 0x280-0x2CF; tiles 0x2D0-0x2EF still hold
    // the world map's font (0x08015F10: LZ77 0x08140CD8[lang], its 4 KiB at +0x3000 copied
    // to tile 0x280). 0x08067F94 then puts the language's course-clear glyphs at tile 0x2B0.
    let mut chars = m.read(BG_VRAM, 0x1_0000)?;
    let map_font = rom.decompress(rom.ptr(MAP_FONTS + LANG * 4)?)?;
    let (from, to) = (MAP_FONT_OFFSET + (0x2D0 - 0x280) * 32, 0x2D0 * 32);
    chars[to..to + 32 * 32].copy_from_slice(&map_font[from..from + 32 * 32]);
    let font = rom.decompress(rom.ptr(CLEAR_FONTS + LANG * 4)?)?;
    chars[0x5600..0x5600 + font.len()].copy_from_slice(&font);
    let sheet = Sheet { chars: &chars, palette: parse_palette(&m.read(BG_PAL_BUFFER, 0x200)?) };
    let render = |ids: &[u32], (cx, cy): (u32, u32), (w, h): (u32, u32), path: std::path::PathBuf| {
        let mut map = vec![None; 32 * 32];
        for &id in ids {
            apply_text(rom, rom.ptr(TEXTS + (LANG * 3 + id - 0xAE) * 4)?, &mut map)?;
        }
        let mut px = vec![0u8; (w * 8 * h * 8 * 4) as usize];
        for ty in 0..h {
            for tx in 0..w {
                if let Some((tile, pal, hflip, vflip)) = map[((cy + ty) * 32 + cx + tx) as usize] {
                    sheet.tile_flip(&mut px, w * 8, tile, tx * 8, ty * 8, hflip, vflip, pal);
                }
            }
        }
        Rgba { width: w * 8, height: h * 8, pixels: px }.write_png(&path).map_err(|e| ExtractError(e.to_string()))
    };
    render(&[0xAF], COURSE_CLEAR_AT, COURSE_CLEAR_TILES, course_clear_path(out_dir))?;
    render(&[0xB0], CARD_GOT_AT, CARD_GOT_TILES, card_got_path(out_dir))?;
    println!("  course clear texts");
    Ok(())
}

/// Growth item, object type 0x0D (init 0x0804E480, main 0x0804EA38): drawn by
/// 0x080475D4 as two 8x16 OBJs, tile 0x88 and its mirror, palette 1. It never changes frame.
const GROWTH_ITEM: u16 = 0x0D;
const FN_DRAW_ITEM: u32 = 0x0804_75D4;

pub fn extract_growth_item(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    let pose = Pose { kind: GROWTH_ITEM, state: 2, draw: FN_DRAW_ITEM, bytes: &[], globals: &[] };
    let cell = oam::render(m, rom.bytes(), &[pose])?;
    // Cut the object's 16x16 box out of the cell; object sprites sit 1 px below the box
    // (OAM y = y + 1 in 0x0804821C and friends).
    let (ox, oy) = (oam::ORIGIN.0 as u32, oam::ORIGIN.1 as u32 + 1);
    let mut px = Vec::with_capacity(16 * 16 * 4);
    for y in oy..oy + 16 {
        let o = ((y * cell.width + ox) * 4) as usize;
        px.extend_from_slice(&cell.pixels[o..o + 16 * 4]);
    }
    Rgba { width: 16, height: 16, pixels: px }
        .write_png(&growth_item_path(out_dir))
        .map_err(|e| ExtractError(e.to_string()))?;
    println!("  growth item");
    Ok(())
}

/// Walker frames side by side in 16x16 cells.
pub fn extract_walker(rom: &Rom, m: &Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    let chars = m.read(OBJ_VRAM, 0x8000)?;
    let sheet = Sheet { chars: &chars, palette: parse_palette(&m.read(OBJ_PAL_BUFFER, 0x200)?) };
    let tiles = rom.ptr(OBJ_FRAME_TILES + WALKER * 4)?;
    let pal = (rom.u8(OBJ_PALETTES + WALKER)? & 7) as u32;
    let (w, h) = (16 * WALKER_FRAMES.len() as u32, 16);
    let mut px = vec![0u8; (w * h * 4) as usize];
    for (i, &f) in WALKER_FRAMES.iter().enumerate() {
        let ox = i as u32 * 16;
        for half in 0..2 {
            let tile = (rom.u16(tiles + f * 4 + half * 2)? & 0x3FF) as u32;
            for dy in 0..2 {
                sheet.tile(&mut px, w, tile + dy * TILES_PER_ROW, ox + half * 8, dy * 8, half == 1, pal);
            }
        }
    }
    Rgba { width: w, height: h, pixels: px }
        .write_png(&walker_path(out_dir))
        .map_err(|e| ExtractError(e.to_string()))?;
    println!("  walker: {} frames", WALKER_FRAMES.len());
    Ok(())
}

/// The ROM pose for every slot of `sma4_assets::PLAYER_*` for one form.
fn player_poses(rom: &Rom, form: u8) -> Result<Vec<u8>, ExtractError> {
    let f = form as u32;
    let table = |base: u32, start: u32, n: u32| -> Result<Vec<u8>, ExtractError> {
        (start..start + n).map(|i| Ok(rom.u8(base + i)?)).collect()
    };
    let walk = table(WALK_POSES, f * 4, 4)?;
    let jump = rom.u8(JUMP_POSES + f)?;
    let run = table(RUN_POSES, [4, 0, 0, 8][form as usize], 4)?;
    let mut poses = walk.clone();
    poses.extend(run);
    if form == RACCOON {
        let air = table(RACCOON_AIR_POSES, 0, 12)?;
        poses.push(air[6]); // jump
        poses.push(air[9]); // falling
        poses.extend(&air[0..3]); // flying up
        poses.extend(&air[3..6]); // flying down
        poses.extend(&air[9..12]); // gliding (falling with the tail wagging)
        poses.extend(table(WHIP_POSES, 0, 10)?);
    } else {
        poses.push(jump);
        poses.push(if form == 1 { BIG_FALL_POSE } else { jump });
        poses.extend([jump; 9]);
        poses.extend([walk[2]; 10]);
    }
    poses.push(DEATH_POSE);
    debug_assert_eq!(poses.len(), PLAYER_FRAMES as usize);
    Ok(poses)
}

/// Player frames for the small, big and raccoon forms (`sma4_assets::PLAYER_*` slots),
/// rendered by running the game's own per-frame graphics upload. Overwrites OBJ tiles 0-3
/// of rows 0-3.
pub fn extract_player(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    let palette = parse_palette(&m.read(OBJ_PAL_BUFFER, 0x200)?);
    m.w32(PLAYER_PTR, PLAYER_SCRATCH)?;
    m.w32(PLAYER_GFX_PTR, PLAYER_GFX_SCRATCH)?;
    m.w8(CHARACTER, 0)?;
    for form in [0u8, 1, RACCOON] {
        let poses = player_poses(rom, form)?;
        let w = PLAYER_W * PLAYER_FRAMES;
        let mut px = vec![0u8; (w * PLAYER_H * 4) as usize];
        for (i, &pose) in poses.iter().enumerate() {
            m.w8(PLAYER_SCRATCH + 0x37, form)?;
            m.w8(PLAYER_SCRATCH + 0x38, pose)?;
            m.call(FN_PLAYER_POSE_GFX, &[])?;
            m.call(FN_PLAYER_DMA, &[])?;
            let chars = m.read(OBJ_VRAM, 4 * TILES_PER_ROW as usize * 32)?;
            let sheet = Sheet { chars: &chars, palette: palette.clone() };
            for ty in 0..4 {
                for tx in 0..4 {
                    sheet.tile(&mut px, w, ty * TILES_PER_ROW + tx, i as u32 * PLAYER_W + tx * 8, ty * 8, false, PLAYER_PALETTE);
                }
            }
        }
        Rgba { width: w, height: PLAYER_H, pixels: px }
            .write_png(&player_path(out_dir, form))
            .map_err(|e| ExtractError(e.to_string()))?;
        println!("  player form {form}: {} poses", poses.len());
    }
    Ok(())
}

/// Super Leaf, object type 0x1E (init 0x0804E5A0, main 0x0804EBE4): drawn by 0x08047400 as
/// two 8x16 OBJs, tiles 0x8E/0x8F (frame 0 of 0x0816B4A0), palette 1; h-flipped while it
/// swings right.
const LEAF: u16 = 0x1E;
const FN_DRAW_LEAF: u32 = 0x0804_7400;

pub fn extract_leaf(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    let pose = Pose { kind: LEAF, state: 2, draw: FN_DRAW_LEAF, bytes: &[], globals: &[] };
    let cell = oam::render(m, rom.bytes(), &[pose])?;
    let (ox, oy) = (oam::ORIGIN.0 as u32, oam::ORIGIN.1 as u32 + 1);
    let mut px = Vec::with_capacity(16 * 16 * 4);
    for y in oy..oy + 16 {
        let o = ((y * cell.width + ox) * 4) as usize;
        px.extend_from_slice(&cell.pixels[o..o + 16 * 4]);
    }
    Rgba { width: 16, height: 16, pixels: px }
        .write_png(&leaf_path(out_dir))
        .map_err(|e| ExtractError(e.to_string()))?;
    println!("  leaf");
    Ok(())
}

/// Renders the coin spin frames side by side, each in a 16x16 cell, aligned the way the
/// game draws them (16-wide frame at x - 4, 8-wide frames at x).
pub fn extract_coin(rom: &Rom, m: &Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    let chars = m.read(OBJ_VRAM, 0x8000)?;
    let sheet = Sheet { chars: &chars, palette: parse_palette(&m.read(OBJ_PAL_BUFFER, 0x200)?) };
    let (w, h) = (16 * COIN_FRAMES, 16);
    let mut px = vec![0u8; (w * h * 4) as usize];
    for f in 0..COIN_FRAMES {
        let tile = rom.u8(COIN_TILES + f)? as u32;
        let flip = rom.u8(COIN_FLIPS + f)? & 1 != 0;
        let ox = f * 16;
        if f == 0 {
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                sheet.tile(&mut px, w, tile + dx + dy * TILES_PER_ROW, ox + dx * 8, dy * 8, false, 0);
            }
        } else {
            for dy in 0..2 {
                sheet.tile(&mut px, w, tile + dy * TILES_PER_ROW, ox + 4, dy * 8, flip, 0);
            }
        }
    }
    Rgba { width: w, height: h, pixels: px }
        .write_png(&coin_spin_path(out_dir))
        .map_err(|e| ExtractError(e.to_string()))?;
    println!("  coin spin: {COIN_FRAMES} frames");
    Ok(())
}

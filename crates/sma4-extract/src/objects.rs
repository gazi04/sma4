//! Object (sprite) graphics, rendered from OBJ VRAM and OBJ palette RAM as the level
//! loaders leave them in the emulated machine.

use sma4_assets::{
    PLAYER_FRAMES, PLAYER_H, PLAYER_W, Rgba, coin_spin_path, growth_item_path, koopa_path,
    leaf_path, player_path, sparkle_path, walker_path,
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

/// Skid dust (0x0808C270): one 8x8 OBJ per age step, tiles u16 0x08170A88[age >> 1],
/// palette 0.
const SKID_DUST_TILES: u32 = 0x0817_0A88;

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
/// Skid pose 0x0813DE41[form != 0] (0x08097800).
const SKID_POSES: u32 = 0x0813_DE41;
/// Death pose (0x080A8FC0 draws 0x4B with the form forced to small).
const DEATH_POSE: u8 = 0x4B;
/// Sliding down a slope (0x08097A94): small, and the other forms.
const SLIDE_POSE_SMALL: u8 = 0x33;
const SLIDE_POSE: u8 = 0x31;
/// Crouching (+0x48), as seen in mGBA; the small form cannot crouch.
const CROUCH_POSE: u8 = 0x0F;
const CROUCH_POSE_RACCOON: u8 = 0x07;
/// The raccoon crouched in the air (0x080981C4): by the tail wag timer +0x65 >> 2.
const RACCOON_CROUCH_AIR_POSES: u32 = 0x0813_DE9B;
/// Grow / shrink poses (0x080A8000), one per 4 frames of player +0x3B.
const GROW_POSES: u32 = 0x0813_DE2F;
/// Fire Mario's throw poses 0x0813DE9E[(+0x67 >> 2) (+3 in the air)] (0x08098648).
const THROW_POSES: u32 = 0x0813_DE9E;
/// Swimming (0x08097724): a stroke's 6 steps, then paddling in place (4); the small form's
/// poses follow the others' in each table.
const SWIM_POSES: u32 = 0x0813_DE43;
const SWIM_IDLE_POSES: u32 = 0x0813_DE4F;
const FIRE: u8 = 2;
/// Player palette loader (0x0809FB54: OBJ palette 7 and 0x02000800 = 0x08173050[form 2,
/// else the character]) and the star palette step (0x08009208: palette 7 = 0x08133210[+0x25
/// & 3], 0 being the player's own at 0x02000800).
const FN_PLAYER_PALETTE: u32 = 0x0809_FB54;
const FN_STAR_PALETTE: u32 = 0x0800_9208;

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
    fn tile_flip(
        &self,
        out: &mut [u8],
        w: u32,
        tile: u32,
        ox: u32,
        oy: u32,
        hflip: bool,
        vflip: bool,
        pal: u32,
    ) {
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
    let sheet = Sheet {
        chars: &chars,
        palette: parse_palette(&m.read(OBJ_PAL_BUFFER, 0x200)?),
    };
    std::fs::write(out_dir.join("obj_pal.bin"), m.read(OBJ_PAL_BUFFER, 0x200)?)
        .map_err(|e| ExtractError(e.to_string()))?;
    // BG VRAM too (64 KiB = 2048 tiles), with the BG palettes.
    let bg = m.read(0x0600_0000, 0x1_0000)?;
    let bg_sheet = Sheet {
        chars: &bg,
        palette: parse_palette(&m.read(0x0200_0000, 0x200)?),
    };
    std::fs::write(out_dir.join("bg_pal.bin"), m.read(0x0200_0000, 0x200)?)
        .map_err(|e| ExtractError(e.to_string()))?;
    for pal in 0..16 {
        let (w, h) = (256, 512);
        let mut px = vec![0u8; (w * h * 4) as usize];
        for t in 0..2048 {
            bg_sheet.tile(&mut px, w, t, (t % 32) * 8, (t / 32) * 8, false, pal);
        }
        Rgba {
            width: w,
            height: h,
            pixels: px,
        }
        .write_png(&out_dir.join(format!("bg_vram_pal{pal:x}.png")))
        .map_err(|e| ExtractError(e.to_string()))?;
    }
    let (w, h) = (256, 256);
    for pal in 0..16 {
        let mut px = vec![0u8; (w * h * 4) as usize];
        for t in 0..1024 {
            sheet.tile(&mut px, w, t, (t % 32) * 8, (t / 32) * 8, false, pal);
        }
        Rgba {
            width: w,
            height: h,
            pixels: px,
        }
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
    let template: Vec<u16> = map
        .chunks_exact(2)
        .take(HUD_COLS * HUD_ROWS)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    let digits = std::array::from_fn(|i| rom.u16(HUD_DIGITS + i as u32 * 2).unwrap_or(0xFF));
    let icon = [rom.u16(HUD_ICON)?, rom.u16(HUD_ICON + 2)?];
    HudAsset {
        template,
        digits,
        icon,
    }
    .write(out_dir)
    .map_err(|e| ExtractError(e.to_string()))?;

    let chars = m.read(BG_VRAM, 0x1_0000)?;
    let sheet = Sheet {
        chars: &chars,
        palette: parse_palette(&m.read(BG_PAL_BUFFER, 0x200)?),
    };
    let w = 8 * 2 * HUD_TILES as u32;
    let mut px = vec![0u8; (w * 8 * 4) as usize];
    for (half, pal) in [(0u32, 0u32), (1, 15)] {
        for i in 0..HUD_TILES as u32 {
            let cell = half * HUD_TILES as u32 + i;
            sheet.tile(
                &mut px,
                w,
                HUD_TILE_BASE as u32 + i,
                cell * 8,
                0,
                false,
                pal,
            );
        }
    }
    Rgba {
        width: w,
        height: 8,
        pixels: px,
    }
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

pub fn extract_score_popups(
    rom: &Rom,
    m: &Machine,
    out_dir: &std::path::Path,
) -> Result<(), ExtractError> {
    use sma4_assets::{SCORE_POPUP_TYPES, score_popups_path};
    let chars = m.read(OBJ_VRAM, 0x8000)?;
    let sheet = Sheet {
        chars: &chars,
        palette: parse_palette(&m.read(OBJ_PAL_BUFFER, 0x200)?),
    };
    let w = 16 * SCORE_POPUP_TYPES as u32;
    let mut px = vec![0u8; (w * 8 * 4) as usize];
    for t in 0..SCORE_POPUP_TYPES as u32 {
        let pal = 0;
        let left = rom.u16(POPUP_LEFT + t * 2)?;
        if left != 0xFF {
            sheet.tile(&mut px, w, (left & 0x3FF) as u32, t * 16, 0, false, pal);
        }
        let right = rom.u16(POPUP_RIGHT + t * 2)?;
        sheet.tile(
            &mut px,
            w,
            (right & 0x3FF) as u32,
            t * 16 + 8,
            0,
            false,
            pal,
        );
    }
    Rgba {
        width: w,
        height: 8,
        pixels: px,
    }
    .write_png(&score_popups_path(out_dir))
    .map_err(|e| ExtractError(e.to_string()))?;

    use sma4_assets::{LIFE_POPUP_TYPES, life_popups_path};
    let w = 32 * LIFE_POPUP_TYPES as u32;
    let mut px = vec![0u8; (w * 16 * 4) as usize];
    for t in 0..LIFE_POPUP_TYPES as u32 {
        let tile = (rom.u16(LIFE_POPUP_TILES + t * 2)? & 0x3FF) as u32;
        for ty in 0..2 {
            for tx in 0..4 {
                sheet.tile(
                    &mut px,
                    w,
                    tile + ty * TILES_PER_ROW + tx,
                    t * 32 + tx * 8,
                    ty * 8,
                    false,
                    1,
                );
            }
        }
    }
    Rgba {
        width: w,
        height: 16,
        pixels: px,
    }
    .write_png(&life_popups_path(out_dir))
    .map_err(|e| ExtractError(e.to_string()))?;
    println!("  score popups");
    Ok(())
}

/// Sparkle frames side by side in 8x8 cells, smallest first.
/// Brick debris (effect kind 2, draw 0x080907F0): 8x8 OBJ tile 0xA5, palette 1.
const DEBRIS_TILE: u32 = 0xA5;
const DEBRIS_PALETTE: u32 = 1;

pub fn extract_brick_debris(m: &Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    let chars = m.read(OBJ_VRAM, 0x8000)?;
    let sheet = Sheet { chars: &chars, palette: parse_palette(&m.read(OBJ_PAL_BUFFER, 0x200)?) };
    let mut px = vec![0u8; 8 * 8 * 4];
    sheet.tile(&mut px, 8, DEBRIS_TILE, 0, 0, false, DEBRIS_PALETTE);
    Rgba { width: 8, height: 8, pixels: px }
        .write_png(&sma4_assets::brick_debris_path(out_dir))
        .map_err(|e| ExtractError(e.to_string()))?;
    println!("  brick debris");
    Ok(())
}

pub fn extract_sparkle(m: &Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    let chars = m.read(OBJ_VRAM, 0x8000)?;
    let sheet = Sheet {
        chars: &chars,
        palette: parse_palette(&m.read(OBJ_PAL_BUFFER, 0x200)?),
    };
    let w = 8 * SPARKLE_TILES;
    let mut px = vec![0u8; (w * 8 * 4) as usize];
    for i in 0..SPARKLE_TILES {
        sheet.tile(
            &mut px,
            w,
            SPARKLE_TILE + i,
            i * 8,
            0,
            false,
            SPARKLE_PALETTE,
        );
    }
    Rgba {
        width: w,
        height: 8,
        pixels: px,
    }
    .write_png(&sparkle_path(out_dir))
    .map_err(|e| ExtractError(e.to_string()))?;
    println!("  sparkle: {SPARKLE_TILES} frames");
    Ok(())
}

/// Skid dust frames side by side in 8x8 cells.
pub fn extract_skid_dust(
    rom: &Rom,
    m: &Machine,
    out_dir: &std::path::Path,
) -> Result<(), ExtractError> {
    use sma4_assets::{SKID_DUST_FRAMES, skid_dust_path};
    let chars = m.read(OBJ_VRAM, 0x8000)?;
    let sheet = Sheet {
        chars: &chars,
        palette: parse_palette(&m.read(OBJ_PAL_BUFFER, 0x200)?),
    };
    let w = 8 * SKID_DUST_FRAMES;
    let mut px = vec![0u8; (w * 8 * 4) as usize];
    for i in 0..SKID_DUST_FRAMES {
        let tile = (rom.u16(SKID_DUST_TILES + i * 2)? & 0x3FF) as u32;
        sheet.tile(&mut px, w, tile, i * 8, 0, false, 0);
    }
    Rgba {
        width: w,
        height: 8,
        pixels: px,
    }
    .write_png(&skid_dust_path(out_dir))
    .map_err(|e| ExtractError(e.to_string()))?;
    println!("  skid dust: {SKID_DUST_FRAMES} frames");
    Ok(())
}

/// Water splash tiles by step (0x0808A128): u16 OBJ tile per (timer >> 3).
const SPLASH_TILES: u32 = 0x0817_09BA;
/// Air bubble OBJ tile (0x08089E9C).
const BUBBLE_TILE: u32 = 0x223;
/// Palette of the splash and the bubbles (attribute 2 = 0x14xx).
const WATER_PALETTE: u32 = 1;

/// The water splash (`sma4_assets::splash_path`) and the air bubble, from OBJ VRAM as a
/// water level leaves it.
pub fn extract_water(rom: &Rom, m: &Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    use sma4_assets::{SPLASH_FRAMES, bubble_path, splash_path};
    let chars = m.read(OBJ_VRAM, 0x8000)?;
    let sheet = Sheet {
        chars: &chars,
        palette: parse_palette(&m.read(OBJ_PAL_BUFFER, 0x200)?),
    };
    let w = 16 * SPLASH_FRAMES;
    let mut px = vec![0u8; (w * 16 * 4) as usize];
    for i in 0..SPLASH_FRAMES {
        let tile = (rom.u16(SPLASH_TILES + i * 2)? & 0x3FF) as u32;
        for (dy, t) in [(0, tile), (8, tile + 0x20)] {
            sheet.tile(&mut px, w, t, i * 16, dy, false, WATER_PALETTE);
            sheet.tile(&mut px, w, t, i * 16 + 8, dy, true, WATER_PALETTE);
        }
    }
    Rgba { width: w, height: 16, pixels: px }
        .write_png(&splash_path(out_dir))
        .map_err(|e| ExtractError(e.to_string()))?;
    let mut px = vec![0u8; 8 * 8 * 4];
    sheet.tile(&mut px, 8, BUBBLE_TILE, 0, 0, false, WATER_PALETTE);
    Rgba { width: 8, height: 8, pixels: px }
        .write_png(&bubble_path(out_dir))
        .map_err(|e| ExtractError(e.to_string()))?;
    println!("  splash: {SPLASH_FRAMES} frames, bubble");
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

pub fn extract_koopas(
    rom: &Rom,
    m: &mut Machine,
    out_dir: &std::path::Path,
) -> Result<(), ExtractError> {
    for kind in [0x6Cu16, 0x6D] {
        extract_koopa_kind(rom, m, out_dir, kind)?;
    }
    let para: Vec<[(u32, u8); 2]> = [0u8, 2, 4, 6]
        .iter()
        .map(|&c| [(0x17, (c >> 2) & 1), (0x3A, c)])
        .collect();
    let poses: Vec<Pose> = para
        .iter()
        .map(|b| Pose {
            kind: 0x6E,
            state: 2,
            draw: FN_DRAW_WALKER,
            bytes: b,
            globals: &[],
        })
        .collect();
    oam::render(m, rom.bytes(), &poses)?
        .write_png(&koopa_path(out_dir, 0x6E))
        .map_err(|e| ExtractError(e.to_string()))?;
    println!("  paratroopa 0x6e: {} frames", poses.len());
    Ok(())
}

/// Paragoomba 0x73: draw 0x08053D88 = the Goomba body (0x080484C4: frame +0x17, OBJ
/// palette +0x21) and two wings (tile 0x108, or 0x107 while +0x3C bit 3 is set) 10 px up.
/// Stomped, it becomes 0x72 with its palette (+0x21 = 0x0816A770[type] & 7) kept, drawn
/// by 0x080484C4 alone (frame 3 = flattened, state 7). The Micro-Goomba dropper 0x74
/// (main 0x0805425C) uses the same draw.
const PARAGOOMBAS: [u16; 2] = [0x73, 0x74];
const FN_DRAW_PARAGOOMBA: u32 = 0x0805_3D88;
const FN_DRAW_GOOMBA_BODY: u32 = 0x0804_84C4;

pub fn extract_paragoomba(
    rom: &Rom,
    m: &mut Machine,
    out_dir: &std::path::Path,
) -> Result<(), ExtractError> {
    use sma4_assets::{PARAGOOMBA_CELLS, paragoomba_cell, paragoomba_path};
    for kind in PARAGOOMBAS {
        let pal = rom.u8(OBJ_PALETTES + kind as u32)? & 7;
        let mut bytes = Vec::new();
        for body in 0..2u8 {
            for wings_up in [false, true] {
                debug_assert_eq!(paragoomba_cell(body as usize, wings_up), bytes.len());
                bytes.push(vec![(0x17, body), (0x3C, if wings_up { 8 } else { 0 })]);
            }
        }
        for frame in [0u8, 1, 3] {
            bytes.push(vec![(0x17, frame), (0x21, pal)]);
        }
        let poses: Vec<Pose> = bytes
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let (kind, draw) = if i < 4 {
                    (kind, FN_DRAW_PARAGOOMBA)
                } else {
                    (WALKER as u16, FN_DRAW_GOOMBA_BODY)
                };
                Pose {
                    kind,
                    state: 2,
                    draw,
                    bytes: b,
                    globals: &[],
                }
            })
            .collect();
        debug_assert_eq!(poses.len(), PARAGOOMBA_CELLS as usize);
        oam::render(m, rom.bytes(), &poses)?
            .write_png(&paragoomba_path(out_dir, kind))
            .map_err(|e| ExtractError(e.to_string()))?;
        println!("  paragoomba {kind:#x}: {} frames", poses.len());
    }
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

pub fn extract_piranhas(
    rom: &Rom,
    m: &mut Machine,
    out_dir: &std::path::Path,
) -> Result<(), ExtractError> {
    use sma4_assets::{PIRANHA_FRAMES, piranha_path};
    for kind in [0xA0u16, 0xA2] {
        debug_assert_eq!(
            oam::ORIGIN.1 - sma4_assets::PIRANHA_ORIGIN.1,
            64 - PIRANHA_Y as i32
        );
        let bytes: Vec<[(u32, u8); 4]> = [
            (PIRANHA_OUT, 0),
            (PIRANHA_OUT, 8),
            (PIRANHA_LOW, 0),
            (PIRANHA_LOW, 8),
        ]
        .iter()
        .map(|&(home, chomp)| [(0x09, PIRANHA_Y), (0x3A, home), (0x1C, 0x11), (0x3C, chomp)])
        .collect();
        let poses: Vec<Pose> = bytes
            .iter()
            .map(|b| Pose {
                kind,
                state: 2,
                draw: FN_DRAW_PIRANHA,
                bytes: b,
                globals: &[],
            })
            .collect();
        debug_assert_eq!(poses.len(), PIRANHA_FRAMES as usize);
        oam::render(m, rom.bytes(), &poses)?
            .write_png(&piranha_path(out_dir, kind))
            .map_err(|e| ExtractError(e.to_string()))?;
        println!("  piranha {kind:#x}: {} frames", poses.len());
    }
    extract_fire_piranha(rom, m, out_dir)?;
    extract_fireball(rom, m, out_dir)
}

/// The red Piranha Plant hanging from a ceiling pipe (0xA3, World 1-5): the same draw
/// 0x0805564C with +0x1A = 0x80 (init 0x08055140), which flips it; cells as for 0xA2.
pub fn extract_ceiling_piranha(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    use sma4_assets::piranha_path;
    let kind = 0xA3u16;
    let bytes: Vec<[(u32, u8); 5]> = [(PIRANHA_OUT, 0), (PIRANHA_OUT, 8), (PIRANHA_LOW, 0), (PIRANHA_LOW, 8)]
        .iter()
        .map(|&(home, chomp)| [(0x09, PIRANHA_Y), (0x3A, home), (0x1C, 0x11), (0x3C, chomp), (0x1A, 0x80)])
        .collect();
    let poses: Vec<Pose> =
        bytes.iter().map(|b| Pose { kind, state: 2, draw: FN_DRAW_PIRANHA, bytes: b, globals: &[] }).collect();
    oam::render(m, rom.bytes(), &poses)?.write_png(&piranha_path(out_dir, kind)).map_err(|e| ExtractError(e.to_string()))?;
    println!("  piranha {kind:#x}: {} frames", poses.len());
    Ok(())
}

/// Venus Fire Trap 0xA4: the same draw routine, which for fire plants (0x0816E56E bit 7)
/// v-flips the head while the player is above (0x08049FDC: player y +4 - object y < 0).
/// Main sets +0x1A bit 6 (0x0816E62C[side]) when the player is to the right.
const FIRE_PIRANHA: u16 = 0xA4;
/// Scratch player y (+4, 32-bit, 8 fraction bits): above or below the plant.
const PLAYER_Y_BYTE: u32 = PLAYER_SCRATCH + 5;
const ABOVE: u8 = 0;
const BELOW: u8 = 0xA0;

fn extract_fire_piranha(
    rom: &Rom,
    m: &mut Machine,
    out_dir: &std::path::Path,
) -> Result<(), ExtractError> {
    use sma4_assets::{FIRE_PIRANHA_LOOKS, PIRANHA_FRAMES, fire_piranha_cell, piranha_path};
    let mut bytes = Vec::new();
    let mut globals = Vec::new();
    for look in 0..FIRE_PIRANHA_LOOKS as usize {
        let (right, up) = (look & 1 != 0, look & 2 != 0);
        for (home, chomp) in [
            (PIRANHA_OUT, 0),
            (PIRANHA_OUT, 8),
            (PIRANHA_LOW, 0),
            (PIRANHA_LOW, 8),
        ] {
            debug_assert_eq!(
                fire_piranha_cell(right, up, bytes.len() % PIRANHA_FRAMES as usize),
                bytes.len()
            );
            bytes.push([
                (0x09, PIRANHA_Y),
                (0x3A, home),
                (0x1C, 0x11),
                (0x3C, chomp),
                (0x1A, if right { 0x40 } else { 0 }),
            ]);
            let y = if up { ABOVE } else { BELOW };
            globals.push([
                (PLAYER_Y_BYTE - 1, 0),
                (PLAYER_Y_BYTE, y),
                (PLAYER_Y_BYTE + 1, 0),
                (PLAYER_Y_BYTE + 2, 0),
            ]);
        }
    }
    let poses: Vec<Pose> = bytes
        .iter()
        .zip(&globals)
        .map(|(b, g)| Pose {
            kind: FIRE_PIRANHA,
            state: 2,
            draw: FN_DRAW_PIRANHA,
            bytes: b,
            globals: g,
        })
        .collect();
    oam::render(m, rom.bytes(), &poses)?
        .write_png(&piranha_path(out_dir, FIRE_PIRANHA))
        .map_err(|e| ExtractError(e.to_string()))?;
    println!("  piranha {FIRE_PIRANHA:#x}: {} frames", poses.len());
    Ok(())
}

/// Fireball, projectile slot type 5 (draw 0x0808D5F0 on the slot: +0 x, +4 y, +8 / +0x0A
/// y / x speed, +0x0E type, +0x13 counter). 0x08088EAC sets its OAM index to 0x46 +
/// 0x03007848[0xCCD] + [0xCD2]; 0xCCD = 0x20 puts it at entry 102, which the renderer reads.
const FN_DRAW_FIREBALL: u32 = 0x0808_D5F0;
const FIREBALL_TYPE: u8 = 5;
const FIREBALL_OAM: u8 = 0x20;
/// 0x03003AE1 makes 0x08088EAC report "paused" and skip the draw.
const PAUSED: u32 = 0x0300_3AE1;

fn extract_fireball(
    rom: &Rom,
    m: &mut Machine,
    out_dir: &std::path::Path,
) -> Result<(), ExtractError> {
    use sma4_assets::{FIREBALL_FRAMES, fireball_path};
    let mut bytes = Vec::new();
    for vx in [0x000Bu16, 0xFFF5] {
        for f in 0..FIREBALL_FRAMES as u8 {
            let [lo, hi] = vx.to_le_bytes();
            bytes.push([
                (0x05, oam::AT as u8),
                (0x08, 0),
                (0x09, 0),
                (0x0A, lo),
                (0x0B, hi),
                (0x0E, FIREBALL_TYPE),
                (0x13, f * 4),
                (0xCCD, FIREBALL_OAM),
                (0xCD2, 0),
            ]);
        }
    }
    // The draw also runs the player touch test (0x0808CC9C): keep the player far away.
    let globals = [
        (PAUSED, 0),
        (PLAYER_SCRATCH + 1, 0),
        (PLAYER_SCRATCH + 2, 0x08),
    ];
    let poses: Vec<Pose> = bytes
        .iter()
        .map(|b| Pose {
            kind: 0,
            state: 0,
            draw: FN_DRAW_FIREBALL,
            bytes: b,
            globals: &globals,
        })
        .collect();
    oam::render(m, rom.bytes(), &poses)?
        .write_png(&fireball_path(out_dir))
        .map_err(|e| ExtractError(e.to_string()))?;
    println!("  fireball: {} frames", poses.len());
    Ok(())
}

/// Micro-Goomba (projectile type 6, handler 0x0808D95C, draw 0x0808D7E0): one 8x8 OBJ,
/// tile 0x16F, palette 1, h-flipped while 0x03003CF6 bit 4 is set; once shaken off (+0x14
/// negative) it falls upside down in palette 3. Like the fireball, it takes the OAM entry
/// 0x08088EAC hands out (0xCCD = 0x20: entry 102).
const FN_DRAW_MICRO_GOOMBA: u32 = 0x0808_D7E0;
const MICRO_GOOMBA_TYPE: u8 = 6;
const FRAME_COUNTER: u32 = 0x0300_3CF6;

pub fn extract_micro_goomba(
    rom: &Rom,
    m: &mut Machine,
    out_dir: &std::path::Path,
) -> Result<(), ExtractError> {
    use sma4_assets::{MICRO_GOOMBA_CELLS, micro_goomba_path};
    // Cells: falling (flip 0, 1), shaken off.
    let poses_in = [(0u8, 0u8), (0, 0x10), (0xFF, 0)];
    let bytes: Vec<[(u32, u8); 7]> = poses_in
        .iter()
        .map(|&(state, _)| {
            [
                (0x05, oam::AT as u8),
                (0x08, 0),
                (0x09, 0),
                (0x0E, MICRO_GOOMBA_TYPE),
                (0x14, state),
                (0xCCD, FIREBALL_OAM),
                (0xCD2, 0),
            ]
        })
        .collect();
    let globals: Vec<[(u32, u8); 4]> = poses_in
        .iter()
        .map(|&(_, flip)| {
            [
                (PAUSED, 0),
                (PLAYER_SCRATCH + 1, 0),
                (PLAYER_SCRATCH + 2, 0x08),
                (FRAME_COUNTER, flip),
            ]
        })
        .collect();
    let poses: Vec<Pose> = bytes
        .iter()
        .zip(&globals)
        .map(|(b, g)| Pose {
            kind: 0,
            state: 0,
            draw: FN_DRAW_MICRO_GOOMBA,
            bytes: b,
            globals: g,
        })
        .collect();
    debug_assert_eq!(poses.len(), MICRO_GOOMBA_CELLS as usize);
    oam::render(m, rom.bytes(), &poses)?
        .write_png(&micro_goomba_path(out_dir))
        .map_err(|e| ExtractError(e.to_string()))?;
    println!("  micro-goomba: {} frames", poses.len());
    Ok(())
}

/// Boomerang Bro (type 0x82, draw 0x08068BB8: body 8 px below the object's y; frame +0x17
/// = walk step (+0x3B bit 4) + 2 while throwing (+0x23), facing +0x1A bit 6) and its
/// boomerang (projectile type 2, draw 0x0808CEF8: one 16x16 OBJ, tile 0x08170ACC[(+0x13 >>
/// 2) & 3], flips 0x08170AC8[..] ^ the direction, palette 1).
const BOOMERANG_BRO: u16 = 0x82;
const FN_DRAW_BOOMERANG_BRO: u32 = 0x0806_8BB8;
const FN_DRAW_BOOMERANG: u32 = 0x0808_D05C;
const BOOMERANG_TYPE: u8 = 2;

pub fn extract_boomerang_bro(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    use sma4_assets::{BOOMERANG_FRAMES, BRO_CELLS, BRO_RAISE, boomerang_bro_cell, boomerang_bro_path, boomerang_path};
    let raise = (oam::AT - BRO_RAISE) as u8;
    let mut bytes = Vec::new();
    for right in [false, true] {
        for throwing in [false, true] {
            for step in 0..2u8 {
                debug_assert_eq!(boomerang_bro_cell(right, throwing, step as usize), bytes.len());
                bytes.push(vec![(0x09, raise), (0x1A, if right { 0x40 } else { 0 }), (0x3B, step * 0x10), (0x23, throwing as u8)]);
            }
        }
    }
    let poses: Vec<Pose> =
        bytes.iter().map(|b| Pose { kind: BOOMERANG_BRO, state: 2, draw: FN_DRAW_BOOMERANG_BRO, bytes: b, globals: &[] }).collect();
    debug_assert_eq!(poses.len(), BRO_CELLS as usize);
    oam::render(m, rom.bytes(), &poses)?.write_png(&boomerang_bro_path(out_dir)).map_err(|e| ExtractError(e.to_string()))?;
    // The boomerang flying right (speed +), then left.
    let mut bytes = Vec::new();
    for vx in [0x0020u16, 0xFFE0] {
        for f in 0..BOOMERANG_FRAMES as u8 {
            let [lo, hi] = vx.to_le_bytes();
            bytes.push([
                (0x05, oam::AT as u8),
                (0x08, 0),
                (0x09, 0),
                (0x0A, lo),
                (0x0B, hi),
                (0x0E, BOOMERANG_TYPE),
                (0x13, f * 4),
                (0x14, 0x0F),
                (0xCCD, FIREBALL_OAM),
                (0xCD2, 0),
            ]);
        }
    }
    let globals = [(PAUSED, 0)];
    let poses: Vec<Pose> =
        bytes.iter().map(|b| Pose { kind: 0, state: 0, draw: FN_DRAW_BOOMERANG, bytes: b, globals: &globals }).collect();
    oam::render(m, rom.bytes(), &poses)?.write_png(&boomerang_path(out_dir)).map_err(|e| ExtractError(e.to_string()))?;
    println!("  boomerang bro, boomerang");
    Ok(())
}

/// Hammer Bro (type 0x81, main 0x08068CF8): the Boomerang Bro's draw 0x08068BB8 in its own
/// palette; its hammer is projectile type 1, drawn by 0x0808D05C -> 0x0808CDF8.
const HAMMER_BRO: u16 = 0x81;
const HAMMER_TYPE: u8 = 1;
/// The battle chest's item (0x08139FE0[world 1][2]: a Starman).
const BATTLE_ITEM: u8 = 9;

pub fn extract_hammer_bro(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    use sma4_assets::{BRO_CELLS, BRO_RAISE, HAMMER_FRAMES, boomerang_bro_cell, hammer_bro_path, hammer_path};
    let raise = (oam::AT - BRO_RAISE) as u8;
    let mut bytes = Vec::new();
    for right in [false, true] {
        for throwing in [false, true] {
            for step in 0..2u8 {
                debug_assert_eq!(boomerang_bro_cell(right, throwing, step as usize), bytes.len());
                bytes.push(vec![(0x09, raise), (0x1A, if right { 0x40 } else { 0 }), (0x3B, step * 0x10), (0x23, throwing as u8)]);
            }
        }
    }
    let poses: Vec<Pose> =
        bytes.iter().map(|b| Pose { kind: HAMMER_BRO, state: 2, draw: FN_DRAW_BOOMERANG_BRO, bytes: b, globals: &[] }).collect();
    debug_assert_eq!(poses.len(), BRO_CELLS as usize);
    oam::render(m, rom.bytes(), &poses)?.write_png(&hammer_bro_path(out_dir)).map_err(|e| ExtractError(e.to_string()))?;
    // The hammer flying right (speed +0x12, 0x0816F040), then left.
    let mut bytes = Vec::new();
    for vx in [0x0012u16, 0xFFEE] {
        for f in 0..HAMMER_FRAMES as u8 {
            let [lo, hi] = vx.to_le_bytes();
            bytes.push([
                (0x05, oam::AT as u8),
                (0x08, 0),
                (0x09, 0),
                (0x0A, lo),
                (0x0B, hi),
                (0x0E, HAMMER_TYPE),
                (0x13, f * 4),
                (0x14, 0),
                (0xCCD, FIREBALL_OAM),
                (0xCD2, 0),
            ]);
        }
    }
    let globals = [(PAUSED, 0)];
    let poses: Vec<Pose> =
        bytes.iter().map(|b| Pose { kind: 0, state: 0, draw: FN_DRAW_BOOMERANG, bytes: b, globals: &globals }).collect();
    oam::render(m, rom.bytes(), &poses)?.write_png(&hammer_path(out_dir)).map_err(|e| ExtractError(e.to_string()))?;
    // The chest effect 7 makes, and its item.
    let item_draw = if rom.u8(CHEST_ITEM_DRAW + BATTLE_ITEM as u32).map_err(|e| ExtractError(e.to_string()))? & 0x80 != 0 {
        FN_DRAW_ITEM
    } else {
        FN_DRAW_CHEST
    };
    let chest = [(0x17, 0)];
    let item = [(0x17, BATTLE_ITEM)];
    let poses = [
        Pose { kind: 0x52, state: 2, draw: FN_DRAW_CHEST, bytes: &chest, globals: &[] },
        Pose { kind: 0x52, state: 2, draw: item_draw, bytes: &item, globals: &[] },
    ];
    write_sheet(rom, m, out_dir, sma4_assets::BATTLE_CHEST_SHEET, &poses)?;
    println!("  hammer bro, hammer, battle chest");
    Ok(())
}

/// Puff of smoke, object state 8 (0x08044550, entered from state 6 by enemies whose
/// 0x0816A89D class is 7): timer +0x23 starts at 0x1F; two 8x16 OBJs at x and x + 8, tile
/// 0x08169484[timer >> 3], palette 1, the halves h-flipped by turns (0x03003CF6 >> 1).
const FN_PUFF: u32 = 0x0804_4550;
const PUFF_STATE: u8 = 8;

pub fn extract_puff(
    rom: &Rom,
    m: &mut Machine,
    out_dir: &std::path::Path,
) -> Result<(), ExtractError> {
    use sma4_assets::{PUFF_FRAMES, puff_path};
    let timers: Vec<[(u32, u8); 1]> = (0..PUFF_FRAMES as u8)
        .map(|i| [(0x23, 0x1F - i * 8)])
        .collect();
    let poses: Vec<Pose> = timers
        .iter()
        .map(|b| Pose {
            kind: 0xA0,
            state: PUFF_STATE,
            draw: FN_PUFF,
            bytes: b,
            globals: &[(SPIN_COUNTER, 0)],
        })
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
    Rgba {
        width: w,
        height: 16,
        pixels: px,
    }
    .write_png(&puff_path(out_dir))
    .map_err(|e| ExtractError(e.to_string()))?;
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

pub fn extract_goal(
    rom: &Rom,
    m: &mut Machine,
    out_dir: &std::path::Path,
) -> Result<(), ExtractError> {
    use sma4_assets::{GOAL_FRAMES, goal_path};
    let frames: Vec<[(u32, u8); 1]> = (0..GOAL_FRAMES as u8 - 1).map(|f| [(0x17, f)]).collect();
    let mut poses: Vec<Pose> = frames
        .iter()
        .map(|b| Pose {
            kind: GOAL,
            state: 2,
            draw: FN_DRAW_GOAL,
            bytes: b,
            globals: &[],
        })
        .collect();
    let at = oam::AT as u8;
    let box_bytes = [(0x3D, 0xFF), (0x46, at), (0x47, 0), (0x4A, at), (0x4B, 0)];
    poses.push(Pose {
        kind: GOAL,
        state: 2,
        draw: FN_DRAW_GOAL_BOX,
        bytes: &box_bytes,
        globals: &[],
    });
    oam::render(m, rom.bytes(), &poses)?
        .write_png(&goal_path(out_dir))
        .map_err(|e| ExtractError(e.to_string()))?;
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
fn apply_text(
    rom: &Rom,
    mut p: u32,
    map: &mut [Option<(u32, u32, bool, bool)>],
) -> Result<(), ExtractError> {
    let decode = |v: u16| {
        (
            (v & 0x3FF) as u32,
            ((v >> 10) & 7) as u32,
            v & 0x4000 != 0,
            v & 0x8000 != 0,
        )
    };
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

pub fn extract_course_clear(
    rom: &Rom,
    m: &mut Machine,
    out_dir: &std::path::Path,
) -> Result<(), ExtractError> {
    use sma4_assets::{
        CARD_GOT_AT, CARD_GOT_TILES, COURSE_CLEAR_AT, COURSE_CLEAR_TILES, card_got_path,
        course_clear_path,
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
    let sheet = Sheet {
        chars: &chars,
        palette: parse_palette(&m.read(BG_PAL_BUFFER, 0x200)?),
    };
    let render = |ids: &[u32],
                  (cx, cy): (u32, u32),
                  (w, h): (u32, u32),
                  path: std::path::PathBuf| {
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
        Rgba {
            width: w * 8,
            height: h * 8,
            pixels: px,
        }
        .write_png(&path)
        .map_err(|e| ExtractError(e.to_string()))
    };
    render(
        &[0xAF],
        COURSE_CLEAR_AT,
        COURSE_CLEAR_TILES,
        course_clear_path(out_dir),
    )?;
    render(&[0xB0], CARD_GOT_AT, CARD_GOT_TILES, card_got_path(out_dir))?;
    println!("  course clear texts");
    Ok(())
}

/// Growth item, object type 0x0D (init 0x0804E480, main 0x0804EA38): drawn by
/// 0x080475D4 as two 8x16 OBJs, tile 0x88 and its mirror, palette 1. It never changes frame.
const GROWTH_ITEM: u16 = 0x0D;
const FN_DRAW_ITEM: u32 = 0x0804_75D4;

pub fn extract_growth_item(
    rom: &Rom,
    m: &mut Machine,
    out_dir: &std::path::Path,
) -> Result<(), ExtractError> {
    let pose = Pose {
        kind: GROWTH_ITEM,
        state: 2,
        draw: FN_DRAW_ITEM,
        bytes: &[],
        globals: &[],
    };
    let cell = oam::render(m, rom.bytes(), &[pose])?;
    // Cut the object's 16x16 box out of the cell; object sprites sit 1 px below the box
    // (OAM y = y + 1 in 0x0804821C and friends).
    let (ox, oy) = (oam::ORIGIN.0 as u32, oam::ORIGIN.1 as u32 + 1);
    let mut px = Vec::with_capacity(16 * 16 * 4);
    for y in oy..oy + 16 {
        let o = ((y * cell.width + ox) * 4) as usize;
        px.extend_from_slice(&cell.pixels[o..o + 16 * 4]);
    }
    Rgba {
        width: 16,
        height: 16,
        pixels: px,
    }
    .write_png(&growth_item_path(out_dir))
    .map_err(|e| ExtractError(e.to_string()))?;
    println!("  growth item");
    Ok(())
}

/// Walker frames side by side in 16x16 cells.
pub fn extract_walker(
    rom: &Rom,
    m: &Machine,
    out_dir: &std::path::Path,
) -> Result<(), ExtractError> {
    let chars = m.read(OBJ_VRAM, 0x8000)?;
    let sheet = Sheet {
        chars: &chars,
        palette: parse_palette(&m.read(OBJ_PAL_BUFFER, 0x200)?),
    };
    let tiles = rom.ptr(OBJ_FRAME_TILES + WALKER * 4)?;
    let pal = (rom.u8(OBJ_PALETTES + WALKER)? & 7) as u32;
    let (w, h) = (16 * WALKER_FRAMES.len() as u32, 16);
    let mut px = vec![0u8; (w * h * 4) as usize];
    for (i, &f) in WALKER_FRAMES.iter().enumerate() {
        let ox = i as u32 * 16;
        for half in 0..2 {
            let tile = (rom.u16(tiles + f * 4 + half * 2)? & 0x3FF) as u32;
            for dy in 0..2 {
                sheet.tile(
                    &mut px,
                    w,
                    tile + dy * TILES_PER_ROW,
                    ox + half * 8,
                    dy * 8,
                    half == 1,
                    pal,
                );
            }
        }
    }
    Rgba {
        width: w,
        height: h,
        pixels: px,
    }
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
        poses.push(if form == 1 || form == FIRE {
            BIG_FALL_POSE
        } else {
            jump
        });
        poses.extend([jump; 9]);
        poses.extend([walk[2]; 10]);
    }
    poses.push(DEATH_POSE);
    poses.push(rom.u8(SKID_POSES + (form != 0) as u32)?);
    // Throwing (fire form; the others keep a walking step): on the ground, then in the air.
    if form == FIRE {
        poses.extend(table(THROW_POSES, 0, 6)?);
    } else {
        poses.extend([walk[2]; 6]);
    }
    let small = (form == 0) as u32;
    poses.extend(table(SWIM_POSES, small * 6, 6)?);
    poses.extend(table(SWIM_IDLE_POSES, small * 4, 4)?);
    // 0x08097A94.
    poses.push(if form == 0 { SLIDE_POSE_SMALL } else { SLIDE_POSE });
    let crouch = match form {
        0 => walk[2],
        RACCOON => CROUCH_POSE_RACCOON,
        _ => CROUCH_POSE,
    };
    poses.push(crouch);
    if form == RACCOON {
        poses.extend(table(RACCOON_CROUCH_AIR_POSES, 0, 3)?);
    } else {
        poses.extend([crouch; 3]);
    }
    debug_assert_eq!(poses.len(), PLAYER_FRAMES as usize);
    Ok(poses)
}

/// Player frames for the small, big and raccoon forms (`sma4_assets::PLAYER_*` slots),
/// rendered by running the game's own per-frame graphics upload. Overwrites OBJ tiles 0-3
/// of rows 0-3.
pub fn extract_player(
    rom: &Rom,
    m: &mut Machine,
    out_dir: &std::path::Path,
) -> Result<(), ExtractError> {
    use sma4_assets::{PLAYER_STAR_PALETTES, player_star_path};
    let palette = parse_palette(&m.read(OBJ_PAL_BUFFER, 0x200)?);
    m.w32(PLAYER_PTR, PLAYER_SCRATCH)?;
    m.w32(PLAYER_GFX_PTR, PLAYER_GFX_SCRATCH)?;
    m.w8(CHARACTER, 0)?;
    for form in [0u8, 1, FIRE, RACCOON] {
        let poses = player_poses(rom, form)?;
        // The form's own palette (Fire Mario's differs), then the three star palettes.
        m.w8(PLAYER_SCRATCH + 0x37, form)?;
        m.w8(PLAYER_SCRATCH + 0x52, 0)?;
        m.call(FN_PLAYER_PALETTE, &[])?;
        let mut palettes = Vec::new();
        for k in 0..PLAYER_STAR_PALETTES {
            if k > 0 {
                m.w8(PLAYER_SCRATCH + 0x25, k as u8)?;
                m.call(FN_STAR_PALETTE, &[])?;
            }
            let mut p = palette.clone();
            let own = parse_palette(&m.read(OBJ_PAL_BUFFER + PLAYER_PALETTE * 0x20, 0x20)?);
            p[(PLAYER_PALETTE * 16) as usize..][..16].copy_from_slice(&own);
            palettes.push(p);
        }
        for (k, palette) in palettes.iter().enumerate() {
            let w = PLAYER_W * PLAYER_FRAMES;
            let mut px = vec![0u8; (w * PLAYER_H * 4) as usize];
            for (i, &pose) in poses.iter().enumerate() {
                m.w8(PLAYER_SCRATCH + 0x37, form)?;
                m.w8(PLAYER_SCRATCH + 0x38, pose)?;
                m.call(FN_PLAYER_POSE_GFX, &[])?;
                m.call(FN_PLAYER_DMA, &[])?;
                let chars = m.read(OBJ_VRAM, 4 * TILES_PER_ROW as usize * 32)?;
                let sheet = Sheet {
                    chars: &chars,
                    palette: palette.clone(),
                };
                for ty in 0..4 {
                    for tx in 0..4 {
                        sheet.tile(
                            &mut px,
                            w,
                            ty * TILES_PER_ROW + tx,
                            i as u32 * PLAYER_W + tx * 8,
                            ty * 8,
                            false,
                            PLAYER_PALETTE,
                        );
                    }
                }
            }
            let path = if k == 0 {
                player_path(out_dir, form)
            } else {
                player_star_path(out_dir, form, k as u32)
            };
            Rgba {
                width: w,
                height: PLAYER_H,
                pixels: px,
            }
            .write_png(&path)
            .map_err(|e| ExtractError(e.to_string()))?;
        }
        println!(
            "  player form {form}: {} poses, {PLAYER_STAR_PALETTES} palettes",
            poses.len()
        );
    }
    // Back to the small form's palette for what follows.
    m.w8(PLAYER_SCRATCH + 0x37, 0)?;
    m.call(FN_PLAYER_PALETTE, &[])?;
    // Grow / shrink: 0x080A8000 draws pose 0x0813DE2F[step] with the form forced to big.
    use sma4_assets::{PLAYER_GROW_STEPS, player_grow_path};
    let w = PLAYER_W * PLAYER_GROW_STEPS;
    let mut px = vec![0u8; (w * PLAYER_H * 4) as usize];
    for i in 0..PLAYER_GROW_STEPS {
        m.w8(PLAYER_SCRATCH + 0x37, 1)?;
        m.w8(PLAYER_SCRATCH + 0x38, rom.u8(GROW_POSES + i)?)?;
        m.call(FN_PLAYER_POSE_GFX, &[])?;
        m.call(FN_PLAYER_DMA, &[])?;
        let chars = m.read(OBJ_VRAM, 4 * TILES_PER_ROW as usize * 32)?;
        let sheet = Sheet {
            chars: &chars,
            palette: palette.clone(),
        };
        for ty in 0..4 {
            for tx in 0..4 {
                sheet.tile(
                    &mut px,
                    w,
                    ty * TILES_PER_ROW + tx,
                    i * PLAYER_W + tx * 8,
                    ty * 8,
                    false,
                    PLAYER_PALETTE,
                );
            }
        }
    }
    Rgba {
        width: w,
        height: PLAYER_H,
        pixels: px,
    }
    .write_png(&player_grow_path(out_dir))
    .map_err(|e| ExtractError(e.to_string()))?;
    println!("  player grow: {PLAYER_GROW_STEPS} steps");
    Ok(())
}

/// Super Leaf, object type 0x1E (init 0x0804E5A0, main 0x0804EBE4): drawn by 0x08047400 as
/// two 8x16 OBJs, tiles 0x8E/0x8F (frame 0 of 0x0816B4A0), palette 1; h-flipped while it
/// swings right.
const LEAF: u16 = 0x1E;
const FN_DRAW_LEAF: u32 = 0x0804_7400;

/// Fire Flower (0x19) and Starman (0x0C), drawn by the item draw like the mushroom: one
/// 16x16 image each (frame +0x17 = 0; the star of a star block has palette
/// 0x0816E3A0[0] = 0).
pub fn extract_flower_and_star(
    rom: &Rom,
    m: &mut Machine,
    out_dir: &std::path::Path,
) -> Result<(), ExtractError> {
    use sma4_assets::{fire_flower_path, star_path};
    for (kind, path, extra) in [
        (0x19u16, fire_flower_path(out_dir), None),
        (0x0C, star_path(out_dir), Some((0x21u32, 0u8))),
    ] {
        let bytes: Vec<(u32, u8)> = extra.into_iter().collect();
        let pose = Pose {
            kind,
            state: 2,
            draw: FN_DRAW_ITEM,
            bytes: &bytes,
            globals: &[],
        };
        let cell = oam::render(m, rom.bytes(), &[pose])?;
        let (ox, oy) = (oam::ORIGIN.0 as u32, oam::ORIGIN.1 as u32 + 1);
        let mut px = Vec::with_capacity(16 * 16 * 4);
        for y in oy..oy + 16 {
            let o = ((y * cell.width + ox) * 4) as usize;
            px.extend_from_slice(&cell.pixels[o..o + 16 * 4]);
        }
        Rgba {
            width: 16,
            height: 16,
            pixels: px,
        }
        .write_png(&path)
        .map_err(|e| ExtractError(e.to_string()))?;
    }
    println!("  fire flower, star");
    Ok(())
}

pub fn extract_leaf(
    rom: &Rom,
    m: &mut Machine,
    out_dir: &std::path::Path,
) -> Result<(), ExtractError> {
    let pose = Pose {
        kind: LEAF,
        state: 2,
        draw: FN_DRAW_LEAF,
        bytes: &[],
        globals: &[],
    };
    let cell = oam::render(m, rom.bytes(), &[pose])?;
    let (ox, oy) = (oam::ORIGIN.0 as u32, oam::ORIGIN.1 as u32 + 1);
    let mut px = Vec::with_capacity(16 * 16 * 4);
    for y in oy..oy + 16 {
        let o = ((y * cell.width + ox) * 4) as usize;
        px.extend_from_slice(&cell.pixels[o..o + 16 * 4]);
    }
    Rgba {
        width: 16,
        height: 16,
        pixels: px,
    }
    .write_png(&leaf_path(out_dir))
    .map_err(|e| ExtractError(e.to_string()))?;
    println!("  leaf");
    Ok(())
}

/// Renders the coin spin frames side by side, each in a 16x16 cell, aligned the way the
/// game draws them (16-wide frame at x - 4, 8-wide frames at x).
pub fn extract_coin(rom: &Rom, m: &Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    let chars = m.read(OBJ_VRAM, 0x8000)?;
    let sheet = Sheet {
        chars: &chars,
        palette: parse_palette(&m.read(OBJ_PAL_BUFFER, 0x200)?),
    };
    let (w, h) = (16 * COIN_FRAMES, 16);
    let mut px = vec![0u8; (w * h * 4) as usize];
    for f in 0..COIN_FRAMES {
        let tile = rom.u8(COIN_TILES + f)? as u32;
        let flip = rom.u8(COIN_FLIPS + f)? & 1 != 0;
        let ox = f * 16;
        if f == 0 {
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                sheet.tile(
                    &mut px,
                    w,
                    tile + dx + dy * TILES_PER_ROW,
                    ox + dx * 8,
                    dy * 8,
                    false,
                    0,
                );
            }
        } else {
            for dy in 0..2 {
                sheet.tile(
                    &mut px,
                    w,
                    tile + dy * TILES_PER_ROW,
                    ox + 4,
                    dy * 8,
                    flip,
                    0,
                );
            }
        }
    }
    Rgba {
        width: w,
        height: h,
        pixels: px,
    }
    .write_png(&coin_spin_path(out_dir))
    .map_err(|e| ExtractError(e.to_string()))?;
    println!("  coin spin: {COIN_FRAMES} frames");
    Ok(())
}

/// Toad's message box in a Toad House (object 0x34, phase 0 0x0806E384): eight rows of 0x12
/// BG3 map entries, row r from 0x0816F3CA + 0x0816F436[r] * 2, written one row a frame from
/// map byte 0x4C (0x0300384B/C, +0x40 a row). BG3 (0x03002DAA = 0x1E08: char block 2) sits
/// on top (priority 0, scroll 0x03003760/64 = 0) once Toad's init enables it.
const TOAD_BOX_ROWS: u32 = 0x0816_F3CA;
const TOAD_BOX_ROW_OF: u32 = 0x0816_F436;
const BG3_CHARS: u32 = 0x0600_8000;
/// Message ids 0x0816F3C4[kind] (u16) in the language's text table 0x08244318
/// (0x080AB0CC).
const TOAD_MESSAGES: u32 = 0x0816_F3C4;
const TEXT_TABLES: u32 = 0x0824_4318;

pub fn extract_toad_house(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    use sma4_assets::{TOAD_BOX_H, TOAD_BOX_W, TOAD_TEXT_AT, toad_box_path, toad_message_path, toad_text_path};
    let chars = m.read(BG3_CHARS, 0x8000)?;
    let sheet = Sheet { chars: &chars, palette: parse_palette(&m.read(BG_PAL_BUFFER, 0x200)?) };
    let (w, h) = (TOAD_BOX_W * 8, TOAD_BOX_H * 8);
    let entry = |px: &mut Vec<u8>, e: u16, tx: u32, ty: u32| {
        sheet.tile_flip(px, w, (e & 0x3FF) as u32, tx * 8, ty * 8, e & 0x400 != 0, e & 0x800 != 0, (e >> 12) as u32);
    };
    let mut px = vec![0u8; (w * h * 4) as usize];
    for ty in 0..TOAD_BOX_H {
        let row = TOAD_BOX_ROWS + rom.u8(TOAD_BOX_ROW_OF + ty)? as u32 * 2;
        for tx in 0..TOAD_BOX_W {
            entry(&mut px, rom.u16(row + tx * 2)?, tx, ty);
        }
    }
    let write = |px: Vec<u8>, path: std::path::PathBuf| {
        Rgba { width: w, height: h, pixels: px }.write_png(&path).map_err(|e| ExtractError(e.to_string()))
    };
    write(px.clone(), toad_box_path(out_dir))?;
    // The 1-3 house's message (kind 0), drawn whole over the box; the game reveals it.
    let id = rom.u16(TOAD_MESSAGES)? as u32;
    let mut p = rom.ptr(rom.ptr(TEXT_TABLES + LANG * 4)? + id * 4)?;
    let mut bytes = Vec::new();
    let (mut line, mut col) = (0, 0);
    loop {
        let b = rom.u8(p)?;
        p += 1;
        bytes.push(b);
        match b {
            0xFF => break,
            0xF0 => (line, col) = (line + 1, 0),
            0xFE => col += 1,
            _ => {
                let (tx, ty) = (TOAD_TEXT_AT.0 + col, TOAD_TEXT_AT.1 + line);
                // Text tiles replace the box's (palette 0, no flips).
                for y in 0..8 {
                    let o = (((ty * 8 + y) * w + tx * 8) * 4) as usize;
                    px[o..o + 32].fill(0);
                }
                entry(&mut px, b as u16, tx, ty);
                col += 1;
            }
        }
    }
    write(px, toad_text_path(out_dir))?;
    std::fs::write(toad_message_path(out_dir), &bytes).map_err(|e| ExtractError(e.to_string()))?;
    extract_toad(rom, m, out_dir)?;
    extract_house_item(rom, m, out_dir)?;
    extract_chest_lid(rom, m, out_dir)?;
    println!("  toad house message ({} bytes), toad, warp whistle", bytes.len());
    Ok(())
}

/// The open chest: tile change 0x14 (0x08027E3C) writes the 2x2 BG entries
/// 0x0813AA24[0x13 * 4..] at the chest's x (8 px steps) and top row, over the level.
const CHEST_LID: u32 = 0x0813_AA24 + 0x13 * 8;

fn extract_chest_lid(rom: &Rom, m: &Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    let chars = m.read(BG_VRAM, 0x8000)?;
    let sheet = Sheet { chars: &chars, palette: parse_palette(&m.read(BG_PAL_BUFFER, 0x200)?) };
    let mut px = vec![0u8; 16 * 16 * 4];
    for i in 0..4 {
        let e = rom.u16(CHEST_LID + i * 2)?;
        let (tx, ty) = (i % 2 * 8, i / 2 * 8);
        sheet.tile_flip(&mut px, 16, (e & 0x3FF) as u32, tx, ty, e & 0x400 != 0, e & 0x800 != 0, (e >> 12) as u32);
    }
    Rgba { width: 16, height: 16, pixels: px }
        .write_png(&sma4_assets::chest_lid_path(out_dir))
        .map_err(|e| ExtractError(e.to_string()))
}

/// Toad (type 0x34): the generic draw 0x08047340 (frame +0x17 = 0), facing left then right
/// (+0x1A = 0x40), in `oam::CELL_W` x `oam::CELL_H` cells, `TOAD_RAISE` px up.
const TOAD: u16 = 0x34;
const FN_DRAW_OBJECT: u32 = 0x0804_7340;

fn extract_toad(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    // Drawn 16 px higher in the cell (its sprite reaches below the object's 16x16 box).
    let raise = (oam::AT - sma4_assets::TOAD_RAISE) as u8;
    let bytes = [[(0x09, raise), (0x1A, 0)], [(0x09, raise), (0x1A, 0x40)]];
    let poses: Vec<Pose> =
        bytes.iter().map(|b| Pose { kind: TOAD, state: 2, draw: FN_DRAW_OBJECT, bytes: b, globals: &[] }).collect();
    oam::render(m, rom.bytes(), &poses)?.write_png(&sma4_assets::toad_path(out_dir)).map_err(|e| ExtractError(e.to_string()))
}

/// The chest's item (object 0x35, main 0x0804FFFC draws it itself): two 8x16 OBJs at x and
/// x + 8, tiles 0x0816E43E[item] and 0x0816E44B[item]; 0x0816E458[item] gives the palette
/// (bits 3-1) and the right half's flips (bit 6 h, bit 7 v). Here the Warp Whistle.
const ITEM_TILES_LEFT: u32 = 0x0816_E43E;
const ITEM_TILES_RIGHT: u32 = 0x0816_E44B;
const ITEM_ATTRS: u32 = 0x0816_E458;

fn extract_house_item(rom: &Rom, m: &Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    let item = sma4_assets::HOUSE_ITEM as u32;
    let chars = m.read(OBJ_VRAM, 0x8000)?;
    let sheet = Sheet { chars: &chars, palette: parse_palette(&m.read(OBJ_PAL_BUFFER, 0x200)?) };
    let a = rom.u8(ITEM_ATTRS + item)?;
    let pal = (a >> 1 & 7) as u32;
    let mut px = vec![0u8; 16 * 16 * 4];
    let left = rom.u8(ITEM_TILES_LEFT + item)? as u32;
    let right = rom.u8(ITEM_TILES_RIGHT + item)? as u32;
    let (h, v) = (a & 0x40 != 0, a & 0x80 != 0);
    for (half, tile, hflip, vflip) in [(0, left, false, false), (8, right, h, v)] {
        // 8x16 in 2D mapping: the tile and the one a row below; flipped vertically the
        // halves swap.
        let (top, bottom) = if vflip { (tile + TILES_PER_ROW, tile) } else { (tile, tile + TILES_PER_ROW) };
        sheet.tile_flip(&mut px, 16, top, half, 0, hflip, vflip, pal);
        sheet.tile_flip(&mut px, 16, bottom, half, 8, hflip, vflip, pal);
    }
    Rgba { width: 16, height: 16, pixels: px }
        .write_png(&sma4_assets::house_item_path(out_dir))
        .map_err(|e| ExtractError(e.to_string()))
}

/// Autoscroll scripts: 23 scripts (pointers 0x08170F4C, each ending where the next starts,
/// the last at the table), 9 sub-scripts (0x08171170, likewise).
const SCROLL_STEPS: u32 = 0x0817_0E14;
const SCROLL_COMMANDS: u32 = 0x0817_0F4C;
const SCROLL_DURATIONS: u32 = 0x0817_10E0;
const SCROLL_THEN: u32 = 0x0817_11F1;
const SCROLL_SCRIPTS: u32 = 23;
const SUB_COMMANDS: u32 = 0x0817_1170;
const SUB_DURATIONS: u32 = 0x0817_11C4;
const SUB_SCRIPTS: u32 = 9;

pub fn extract_autoscroll(rom: &Rom, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    use sma4_assets::{AutoscrollScripts, ScrollScript};
    let read = |cmds: u32, durs: u32, then: Option<u32>, n: u32| -> Result<Vec<ScrollScript>, ExtractError> {
        (0..n)
            .map(|i| {
                let start = rom.ptr(cmds + i * 4)?;
                let end = if i + 1 < n { rom.ptr(cmds + (i + 1) * 4)? } else { cmds };
                let len = (end - start) as usize;
                Ok(ScrollScript {
                    commands: rom.slice(start, len)?.to_vec(),
                    durations: rom.slice(rom.ptr(durs + i * 4)?, len)?.to_vec(),
                    then: match then {
                        Some(t) => rom.u8(t + i)?,
                        None => 0,
                    },
                })
            })
            .collect()
    };
    let s = AutoscrollScripts {
        steps: std::array::from_fn(|i| rom.u8(SCROLL_STEPS + i as u32).unwrap_or(0) as i8),
        scripts: read(SCROLL_COMMANDS, SCROLL_DURATIONS, Some(SCROLL_THEN), SCROLL_SCRIPTS)?,
        subs: read(SUB_COMMANDS, SUB_DURATIONS, None, SUB_SCRIPTS)?,
    };
    s.write(out_dir).map_err(|e| ExtractError(e.to_string()))?;
    println!("  autoscroll: {} scripts, {} sub-scripts", s.scripts.len(), s.subs.len());
    Ok(())
}

/// The falling lift of World 1-4 (type 0x36, draw 0x0806B310): six 8x16 OBJs side by side,
/// tiles 0x0816B4A0[0x36] (0x9A, 0x9B x 4, 0x9C).
const LIFT: u16 = 0x36;
const FN_DRAW_LIFT: u32 = 0x0806_B310;

pub fn extract_lift(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    use sma4_assets::{LIFT_CELL, LIFT_ORIGIN, lift_path};
    let pose = Pose { kind: LIFT, state: 2, draw: FN_DRAW_LIFT, bytes: &[], globals: &[] };
    oam::render_one(m, rom.bytes(), &pose, LIFT_CELL, LIFT_ORIGIN)?
        .write_png(&lift_path(out_dir))
        .map_err(|e| ExtractError(e.to_string()))?;
    // Red Paratroopa (0x6F, main 0x08053CDC, draw 0x080539E8): body frame +0x17 =
    // (+0x3A >> 3) & 1, wing tile 0x107 / 0x108 by (+0x3A + 2) & 4; cell k for +0x3A = 2k.
    let bytes: Vec<[(u32, u8); 2]> = (0..RED_PARATROOPA_CELLS).map(|k| [(0x17, ((2 * k) >> 3) & 1), (0x3A, 2 * k)]).collect();
    let poses: Vec<Pose> =
        bytes.iter().map(|b| Pose { kind: RED_PARATROOPA, state: 2, draw: FN_DRAW_WALKER, bytes: b, globals: &[] }).collect();
    oam::render(m, rom.bytes(), &poses)?
        .write_png(&koopa_path(out_dir, RED_PARATROOPA))
        .map_err(|e| ExtractError(e.to_string()))?;
    println!("  lift, red paratroopa");
    Ok(())
}

const RED_PARATROOPA: u16 = 0x6F;
const RED_PARATROOPA_CELLS: u8 = 8;


/// One Koopa-like sheet (walk 0 / 1, shell, waking shell, spin 0-3) for `kind`; also the
/// Buzzy Beetle (0x70, drawn by the same routines), whose graphics only World 1-5 loads.
pub fn extract_koopa_kind(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path, kind: u16) -> Result<(), ExtractError> {
        let walk = |f: u8| [(0x17, f)];
        let (w0, w1) = (walk(0), walk(1));
        let shell = [(0x17, 2), (0x34, 0xFF)];
        let waking = [(0x17, 2), (0x34, 0x02)];
        let spin: Vec<[(u32, u8); 1]> = (0..4).map(|k| [(SPIN_COUNTER, k * 2)]).collect();
        let mut poses = vec![
            Pose {
                kind,
                state: 2,
                draw: FN_DRAW_WALKER,
                bytes: &w0,
                globals: &[],
            },
            Pose {
                kind,
                state: 2,
                draw: FN_DRAW_WALKER,
                bytes: &w1,
                globals: &[],
            },
            Pose {
                kind,
                state: 3,
                draw: FN_DRAW_SHELL,
                bytes: &shell,
                globals: &[],
            },
            Pose {
                kind,
                state: 3,
                draw: FN_DRAW_SHELL,
                bytes: &waking,
                globals: &[],
            },
        ];
        for g in &spin {
            poses.push(Pose {
                kind,
                state: 5,
                draw: FN_DRAW_SPIN,
                bytes: &[],
                globals: g,
            });
        }
        oam::render(m, rom.bytes(), &poses)?
            .write_png(&koopa_path(out_dir, kind))
            .map_err(|e| ExtractError(e.to_string()))?;
        println!("  koopa {kind:#x}: {} frames", poses.len());
    Ok(())
}

/// Mini-fortress draw routines (see `sma4_core::fortress`, `sma4_core::boom_boom`).
const FN_DRAW_ROTO_DISC: u32 = 0x0804_6C50;
const FN_DRAW_DRY_BONES: u32 = 0x0806_EC40;
const FN_DRAW_BOOM_BOOM: u32 = 0x0806_CCDC;
const FN_DRAW_BALL: u32 = 0x0804_7400;
const FN_DRAW_CHEST: u32 = 0x0804_705C;
const FN_EMBER: u32 = 0x0808_F6E4;
const FN_LAVA_SPLASH: u32 = 0x0808_F7E4;
const FN_SPARK: u32 = 0x0808_E480;
const FN_SMOKE: u32 = 0x0808_F568;
/// Projectile slots: type +0x0E; the frame counter's copy 0x03003CF6 picks the sparks'
/// palette; 0x03003754 is the camera y the splash reads.
const SPIN_CLOCK: u32 = 0x0300_3CF6;
const CAMERA_Y2: u32 = 0x0300_3754;
const FRAMES: u32 = 0x0300_3AE8;
/// 0x0816F096[item] bit 7: the item is drawn with 0x080475D4 rather than 0x0804705C.
const CHEST_ITEM_DRAW: u32 = 0x0816_F096;
const WARP_WHISTLE: u8 = 0x0C;

/// A projectile slot at the drawing position (x +0, y +4; +8 / +0xA speeds).
fn projectile(kind: u8, extra: &[(u32, u8)]) -> Vec<(u32, u8)> {
    let mut b = vec![(0x05, oam::AT as u8), (0x08, 0), (0x09, 0), (0x0E, kind), (0xCCD, FIREBALL_OAM), (0xCD2, 0)];
    b.extend_from_slice(extra);
    b
}

fn write_sheet(
    rom: &Rom,
    m: &mut Machine,
    out_dir: &std::path::Path,
    sheet: sma4_assets::FortressSheet,
    poses: &[Pose],
) -> Result<(), ExtractError> {
    debug_assert_eq!(poses.len() as u32, sheet.cells);
    oam::render_cells(m, rom.bytes(), poses, sheet.cell, sheet.origin)?
        .write_png(&sheet.path(out_dir))
        .map_err(|e| ExtractError(e.to_string()))
}

/// The mini-fortress's objects, with level 0x1CB's object graphics loaded.
pub fn extract_fortress(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    use sma4_assets::*;
    let paused = [(PAUSED, 0)];
    // Podoboo: 0x080475D4 with frame +0x17, falling = +0x1A bit 7.
    let bytes: Vec<[(u32, u8); 2]> = (0..6).map(|i| [(0x17, i % 3), (0x1A, if i >= 3 { 0x80 } else { 0 })]).collect();
    let poses: Vec<Pose> = bytes.iter().map(|b| Pose { kind: 0x9E, state: 2, draw: FN_DRAW_ITEM, bytes: b, globals: &[] }).collect();
    write_sheet(rom, m, out_dir, PODOBOO_SHEET, &poses)?;
    // Embers: step +0x13, +0x15 kept non-zero so the step stays.
    let bytes: Vec<Vec<(u32, u8)>> = (0..3).map(|s| projectile(0x17, &[(0x13, s), (0x15, 1)])).collect();
    let poses: Vec<Pose> = bytes.iter().map(|b| Pose { kind: 0, state: 0, draw: FN_EMBER, bytes: b, globals: &paused }).collect();
    write_sheet(rom, m, out_dir, EMBER_SHEET, &poses)?;
    // Lava splash: +0x15 = 4 * step + 1.
    let globals = [(PAUSED, 0), (CAMERA_Y2, 0), (CAMERA_Y2 + 1, 0)];
    let bytes: Vec<Vec<(u32, u8)>> = (0..6).map(|s| projectile(0x18, &[(0x15, s * 4 + 1)])).collect();
    let poses: Vec<Pose> = bytes.iter().map(|b| Pose { kind: 0, state: 0, draw: FN_LAVA_SPLASH, bytes: b, globals: &globals }).collect();
    write_sheet(rom, m, out_dir, LAVA_SPLASH_SHEET, &poses)?;
    // Roto-Disc: frame +0x17 = counter & 1, the counter itself for the flips.
    let globals: Vec<[(u32, u8); 1]> = (0..16).map(|f| [(FRAMES, f)]).collect();
    let bytes: Vec<[(u32, u8); 2]> = (0..16).map(|f| [(0x17, f & 1), (0x2C, f >> 2)]).collect();
    let poses: Vec<Pose> = (0..16)
        .map(|i| Pose { kind: 0x5A, state: 2, draw: FN_DRAW_ROTO_DISC, bytes: &bytes[i], globals: &globals[i] })
        .collect();
    write_sheet(rom, m, out_dir, ROTO_DISC_SHEET, &poses)?;
    // Dry Bones: frame +0x17, facing right = +0x1A bit 6.
    let bytes: Vec<[(u32, u8); 2]> = (0..8).map(|i| [(0x17, i % 4), (0x1A, if i >= 4 { 0x40 } else { 0 })]).collect();
    let poses: Vec<Pose> = bytes.iter().map(|b| Pose { kind: 0x3F, state: 2, draw: FN_DRAW_DRY_BONES, bytes: b, globals: &[] }).collect();
    write_sheet(rom, m, out_dir, DRY_BONES_SHEET, &poses)?;
    // Boom Boom: frame +0x17, facing right = +0x1A bit 6.
    let bytes: Vec<[(u32, u8); 3]> =
        (0..24).map(|i| [(0x17, i % 12), (0x1A, if i >= 12 { 0x40 } else { 0 }), (0x14, 0)]).collect();
    let poses: Vec<Pose> = bytes.iter().map(|b| Pose { kind: 0x4B, state: 2, draw: FN_DRAW_BOOM_BOOM, bytes: b, globals: &[] }).collect();
    write_sheet(rom, m, out_dir, BOOM_BOOM_SHEET, &poses)?;
    let poses = [Pose { kind: 0x4A, state: 2, draw: FN_DRAW_BALL, bytes: &[], globals: &[] }];
    write_sheet(rom, m, out_dir, BALL_SHEET, &poses)?;
    // Sparks: palette ((0x03003CF6 >> 2) + slot) & 3.
    let globals: Vec<[(u32, u8); 2]> = (0..4).map(|k| [(PAUSED, 0), (SPIN_CLOCK, k * 4)]).collect();
    let bytes = projectile(0x0D, &[(0x14, 0), (0x15, 0x3F)]);
    let poses: Vec<Pose> = globals.iter().map(|g| Pose { kind: 0, state: 0, draw: FN_SPARK, bytes: &bytes, globals: g }).collect();
    write_sheet(rom, m, out_dir, SPARK_SHEET, &poses)?;
    // Smoke: life +0x14 (counted down once before the draw), tile 0x08170B64[life >> 3].
    let bytes: Vec<Vec<(u32, u8)>> = (0..3).map(|k| projectile(0x16, &[(0x14, k * 8 + 5)])).collect();
    let poses: Vec<Pose> = bytes.iter().map(|b| Pose { kind: 0, state: 0, draw: FN_SMOKE, bytes: b, globals: &paused }).collect();
    write_sheet(rom, m, out_dir, SMOKE_SHEET, &poses)?;
    println!("  mini-fortress: podoboo, embers, lava splash, roto-disc, dry bones, boom boom, ball, sparks, smoke");
    Ok(())
}

/// The fortress's treasure chest and its item, with level 0x1CC's object graphics loaded.
pub fn extract_fortress_chest(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    let item_draw = if rom.u8(CHEST_ITEM_DRAW + WARP_WHISTLE as u32).map_err(|e| ExtractError(e.to_string()))? & 0x80 != 0 {
        FN_DRAW_ITEM
    } else {
        FN_DRAW_CHEST
    };
    let chest = [(0x17, 0)];
    let item = [(0x17, WARP_WHISTLE)];
    let poses = [
        Pose { kind: 0x52, state: 2, draw: FN_DRAW_CHEST, bytes: &chest, globals: &[] },
        Pose { kind: 0x52, state: 2, draw: item_draw, bytes: &item, globals: &[] },
    ];
    write_sheet(rom, m, out_dir, sma4_assets::CHEST_SHEET, &poses)?;
    println!("  mini-fortress chest");
    Ok(())
}

/// The Advance Coin (type 0xCD, init 0x08076C94, main 0x08077720). Idle it is drawn by
/// 0x08076CD8 (a 16x16 OBJ); once hit, 0x08076F28 draws its spinning banner and
/// 0x080775CC / 0x08077518 the number of coins still to collect; collected (phase 5) the
/// same idle routine draws it in the corner in palette 2. See docs/re-notes.md.
const ADVANCE_COIN: u16 = 0xCD;
const FN_DRAW_ADVANCE_IDLE: u32 = 0x0807_6CD8;
const FN_DRAW_ADVANCE_SPIN: u32 = 0x0807_6F28;
const FN_DRAW_ADVANCE_COUNT: u32 = 0x0807_75CC;
const FN_DRAW_ADVANCE_ICON: u32 = 0x0807_7518;

pub fn extract_advance_coin(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    use sma4_assets::{ADVANCE_COIN_CELL, advance_coin_path};
    // Screen position (0, 0x90), the HUD corner; the idle coin's position at the cell's top-left.
    let (hud, idle_at) = ((64, 64 - 0x90), (0, 0));
    let mut cells: Vec<Rgba> = Vec::new();
    let globals: [(u32, u8); 0] = [];
    // Cell 0: the idle coin (0x08076CD8, phase 0).
    let idle = Pose { kind: ADVANCE_COIN, state: 2, draw: FN_DRAW_ADVANCE_IDLE, bytes: &[], globals: &globals };
    cells.push(oam::render_one(m, rom.bytes(), &idle, ADVANCE_COIN_CELL, idle_at)?);
    // Cells 1-10: the digit 0-9 at the units place (0x080775CC with 0x0300379F - 0x030037A0 = n).
    for n in 0..10u8 {
        let g = [(0x0300_379F, n), (0x0300_37A0, 0)];
        let pose = Pose { kind: ADVANCE_COIN, state: 2, draw: FN_DRAW_ADVANCE_COUNT, bytes: &[], globals: &g };
        cells.push(oam::render_one(m, rom.bytes(), &pose, ADVANCE_COIN_CELL, hud)?);
    }
    // Cell 11: the frame around the number (0x08077518).
    let icon = Pose { kind: ADVANCE_COIN, state: 2, draw: FN_DRAW_ADVANCE_ICON, bytes: &[], globals: &globals };
    cells.push(oam::render_one(m, rom.bytes(), &icon, ADVANCE_COIN_CELL, hud)?);
    // Cells 12-19: the banner (0x08076F28) for +0x23 >> 3 = 0-3, each with 0x03003CF6 bit 1
    // clear and set (it flips every other frame).
    for t in 0..4u8 {
        for flip in [0u8, 2] {
            let b = [(0x23, t * 8), (0x08, 0), (0x09, 0x90)];
            let g = [(0x0300_3CF6, flip)];
            let pose = Pose { kind: ADVANCE_COIN, state: 2, draw: FN_DRAW_ADVANCE_SPIN, bytes: &b, globals: &g };
            cells.push(oam::render_one(m, rom.bytes(), &pose, ADVANCE_COIN_CELL, hud)?);
        }
    }
    // Cell 20: the collected coin (0x08076CD8 in phase 5: tile 0x25E, palette 2) at screen
    // (0x10, 0x90).
    let b = [(0x3A, 5), (0x00, 0), (0x01, 0x10), (0x08, 0), (0x09, 0x90)];
    let pose = Pose { kind: ADVANCE_COIN, state: 2, draw: FN_DRAW_ADVANCE_IDLE, bytes: &b, globals: &globals };
    cells.push(oam::render_one(m, rom.bytes(), &pose, ADVANCE_COIN_CELL, hud)?);
    let (w, h) = ADVANCE_COIN_CELL;
    let mut sheet = vec![0u8; (w * cells.len() as u32 * h * 4) as usize];
    for (i, c) in cells.iter().enumerate() {
        for y in 0..h {
            let src = (y * w * 4) as usize;
            let dst = ((y * w * cells.len() as u32 + i as u32 * w) * 4) as usize;
            sheet[dst..dst + (w * 4) as usize].copy_from_slice(&c.pixels[src..src + (w * 4) as usize]);
        }
    }
    Rgba { width: w * cells.len() as u32, height: h, pixels: sheet }
        .write_png(&advance_coin_path(out_dir))
        .map_err(|e| ExtractError(e.to_string()))?;
    println!("  advance coin: {} cells", cells.len());
    Ok(())
}

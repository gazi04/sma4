//! Screens outside the level: the world map with its status bar and "WORLD n / MARIO x n"
//! panel, and the game-over screen. Both are built by running the ROM's own setup code
//! and reading back VRAM, the BG maps and OAM (see docs/re-notes.md, "World map screen"
//! and "Game over").

use sma4_assets::{
    MapScreenAsset, Rgba, game_over_cursor_path, game_over_menu_path, game_over_path,
};
use sma4_rom::gfx::{Bgr555, parse_palette};

use crate::emu::Machine;
use crate::level::ExtractError;

const PLAYER_PTR: u32 = 0x0300_7818;
const PLAYER_GFX_PTR: u32 = 0x0300_7820;
const MAP_STATE_PTR: u32 = 0x0300_7824;
const OBJ_BASE_PTR: u32 = 0x0300_7848;
const WORLD: u32 = 0x0300_2A69;
const LANGUAGE: u32 = 0x0300_3777;
const BG_CNT: u32 = 0x0300_2DA4;
const OAM_BUF: u32 = 0x0300_2DC8;
const BG_PAL_BUFFER: u32 = 0x0200_0000;
const OBJ_PAL_BUFFER: u32 = 0x0200_0200;
const VRAM: u32 = 0x0600_0000;
const OBJ_VRAM: u32 = 0x0601_0000;
/// BG scroll shadows written to the registers in VBlank (0x0800092C, 0x08000DD0).
const SCROLL: [(u32, u32); 4] = [
    (0x0300_3748, 0x0300_374C),
    (0x0300_3750, 0x0300_3754),
    (0x0300_3758, 0x0300_375C),
    (0x0300_3760, 0x0300_3764),
];

/// World-map init 0x0800BE48 (-> 0x0800B648: graphics, palettes, status bar, map, panel),
/// then one pass of map sub-states 0x11-0x13, which draws the panel's sprites.
const FN_MAP_INIT: u32 = 0x0800_BE48;
const FN_MAP_STEPS: [u32; 3] = [0x0800_DE10, 0x0800_E1C4, 0x0800_E4F4];
/// Game over (mode 1 sub-state 0x29 = 0x080138D0): loads the letter and Mario-head OBJ
/// tiles and palettes.
const FN_GAME_OVER_INIT: u32 = 0x0801_38D0;
/// Letter tiles 0x100 | 0x08135E7C[language][i] (8x16), Mario head tile 8 (16x16).
const GAME_OVER_LETTERS: u32 = 0x100;
const GAME_OVER_HEAD: u32 = 8;

fn color(pal: &[Bgr555], p: u32, i: u8) -> [u8; 4] {
    pal[(p * 16 + i as u32) as usize].to_rgba(255)
}

/// One 8x8 tile of 4bpp `chars` as RGBA, with the map entry's flips and palette.
pub(crate) fn bg_tile(chars: &[u8], pal: &[Bgr555], entry: u16) -> [u8; 256] {
    let (t, hf, vf, p) = (
        (entry & 0x3FF) as usize,
        entry & 0x400 != 0,
        entry & 0x800 != 0,
        (entry >> 12) as u32,
    );
    let mut out = [0u8; 256];
    let d = &chars[(t * 32) % chars.len()..][..32];
    for y in 0..8 {
        for x in 0..8 {
            let (sx, sy) = (if hf { 7 - x } else { x }, if vf { 7 - y } else { y });
            let b = d[sy * 4 + sx / 2];
            let c = if sx & 1 == 1 { b >> 4 } else { b & 0xF };
            if c != 0 {
                out[(y * 8 + x) * 4..][..4].copy_from_slice(&color(pal, p, c));
            }
        }
    }
    out
}

fn read_u16s(m: &Machine, addr: u32, n: usize) -> Result<Vec<u16>, ExtractError> {
    Ok(m.read(addr, n * 2)?
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&b| u16::from_le_bytes(b))
        .collect())
}

/// A BG layer as read back: map entries (row-major, full width), char data, scroll.
type Layer = (Vec<u16>, Vec<u8>, (u16, u16));

/// BG layer `bg` (all of its map blocks), its char block and scroll.
pub(crate) fn layer(m: &Machine, bg: u32) -> Result<Layer, ExtractError> {
    let cnt = read_u16s(m, BG_CNT + bg * 2, 1)?[0];
    let chars = m.read(VRAM + ((cnt as u32 >> 2) & 3) * 0x4000, 0x8000)?;
    let screen = VRAM + ((cnt as u32 >> 8) & 0x1F) * 0x800;
    let size = cnt >> 14;
    let (w, h) = [(32, 32), (64, 32), (32, 64), (64, 64)][size as usize];
    // Full layer, block by block, row-major over (w, h) tiles.
    let mut map = vec![0u16; w * h];
    for ty in 0..h {
        for tx in 0..w {
            let block = (tx / 32) + (ty / 32) * (w / 32);
            let at = screen + block as u32 * 0x800 + ((ty % 32) * 32 + tx % 32) as u32 * 2;
            map[ty * w + tx] = read_u16s(m, at, 1)?[0];
        }
    }
    let (hx, vy) = SCROLL[bg as usize];
    let scroll = (read_u16s(m, hx, 1)?[0], read_u16s(m, vy, 1)?[0]);
    Ok((map, chars, scroll))
}

pub(crate) fn layer_width(m: &Machine, bg: u32) -> Result<usize, ExtractError> {
    let cnt = read_u16s(m, BG_CNT + bg * 2, 1)?[0];
    Ok(if cnt >> 14 & 1 == 1 { 64 } else { 32 })
}

/// Renders the 240x160 screen view of one BG layer (its scroll applied).
pub(crate) fn render_view(
    map: &[u16],
    width: usize,
    chars: &[u8],
    pal: &[Bgr555],
    scroll: (u16, u16),
) -> Rgba {
    let height = map.len() / width;
    let mut px = vec![0u8; 240 * 160 * 4];
    for y in 0..160 {
        for x in 0..240 {
            let lx = (x + scroll.0 as usize) % (width * 8);
            let ly = (y + scroll.1 as usize) % (height * 8);
            let tile = bg_tile(chars, pal, map[(ly / 8) * width + lx / 8]);
            let o = ((ly % 8) * 8 + lx % 8) * 4;
            if tile[o + 3] != 0 {
                px[(y * 240 + x) * 4..][..4].copy_from_slice(&tile[o..o + 4]);
            }
        }
    }
    Rgba {
        width: 240,
        height: 160,
        pixels: px,
    }
}

/// Draws all visible OAM entries onto `canvas` (240x160).
pub(crate) fn draw_oam(
    m: &Machine,
    canvas: &mut Rgba,
    slots: std::ops::Range<u32>,
) -> Result<(), ExtractError> {
    const SIZES: [[(u32, u32); 4]; 3] = [
        [(8, 8), (16, 16), (32, 32), (64, 64)],
        [(16, 8), (32, 8), (32, 16), (64, 32)],
        [(8, 16), (8, 32), (16, 32), (32, 64)],
    ];
    let chars = m.read(OBJ_VRAM, 0x8000)?;
    let pal = parse_palette(&m.read(OBJ_PAL_BUFFER, 0x200)?);
    for i in slots.rev() {
        let e = read_u16s(m, OAM_BUF + i * 8, 3)?;
        let (a0, a1, a2) = (e[0], e[1], e[2]);
        if a0 & 0xFF == 0xA0 || a0 >> 14 > 2 {
            continue;
        }
        let (w, h) = SIZES[(a0 >> 14) as usize][(a1 >> 14) as usize];
        let (mut y, x) = ((a0 & 0xFF) as i32, (a1 & 0x1FF) as i32);
        if y >= 160 {
            y -= 256;
        }
        let (t, p, hf) = ((a2 & 0x3FF) as u32, (a2 >> 12) as u32, a1 & 0x1000 != 0);
        for sy in 0..h {
            for sx in 0..w {
                let tile = t + (sy / 8) * 32 + sx / 8;
                let d = &chars[(tile * 32) as usize % chars.len()..][..32];
                let b = d[((sy % 8) * 4 + (sx % 8) / 2) as usize];
                let c = if sx & 1 == 1 { b >> 4 } else { b & 0xF };
                let (cx, cy) = (
                    x + if hf { (w - 1 - sx) as i32 } else { sx as i32 },
                    y + sy as i32,
                );
                if c != 0 && (0..240).contains(&cx) && (0..160).contains(&cy) {
                    let o = ((cy * 240 + cx) * 4) as usize;
                    canvas.pixels[o..o + 4].copy_from_slice(&color(&pal, p, c));
                }
            }
        }
    }
    Ok(())
}

fn setup(m: &mut Machine) -> Result<(), ExtractError> {
    m.clear_ram()?;
    for (ptr, val) in [
        (PLAYER_PTR, 0x0203_A000),
        (PLAYER_GFX_PTR, 0x0203_B000),
        (MAP_STATE_PTR, 0x0203_C000),
        (OBJ_BASE_PTR, 0x0203_8000),
    ] {
        m.w32(ptr, val)?;
    }
    m.w8(LANGUAGE, 0)?;
    for i in 0..128 {
        m.w16(OAM_BUF + i * 8, 0xA0)?;
    }
    Ok(())
}

/// World map of `world` (0 = World 1): map layer, status bar, panel and panel sprites.
pub fn extract_map(
    m: &mut Machine,
    world: u8,
    out_dir: &std::path::Path,
) -> Result<(), ExtractError> {
    setup(m)?;
    m.w8(WORLD, world)?;
    m.call(FN_MAP_INIT, &[])?;
    for f in FN_MAP_STEPS {
        m.call(f, &[])?;
    }
    let pal = parse_palette(&m.read(BG_PAL_BUFFER, 0x200)?);

    // BG1: the map itself.
    let (map, chars, scroll) = layer(m, 1)?;
    let view = render_view(&map, layer_width(m, 1)?, &chars, &pal, scroll);

    // BG0: status bar (vertical scroll -120 puts its top rows at the screen bottom).
    let (bar_full, bar_chars, bar_scroll) = layer(m, 0)?;
    let bar_w = layer_width(m, 0)?;
    let bar: Vec<u16> = (0..32)
        .flat_map(|r| bar_full[r * bar_w..r * bar_w + 32].to_vec())
        .collect();
    // BG3: the panel.
    let (panel, panel_chars, panel_scroll) = layer(m, 3)?;

    // Tiles the two layers can show: their template cells plus the digits the update
    // routines write (bar: 0x290-0x299 in palettes 0 and 15, blank 0x2AD; panel 0x3F0-0x3F9,
    // blank 0x3FE).
    let mut keys: Vec<(u8, u16)> = Vec::new();
    let mut add = |layer: u8, e: u16| {
        let key = (layer, e & 0xF3FF);
        if !keys.contains(&key) {
            keys.push(key);
        }
    };
    for &e in &bar {
        add(0, e);
    }
    for &e in &panel {
        add(3, e);
    }
    for d in 0..10 {
        add(0, 0x290 + d);
        add(0, 0xF290 + d);
        add(3, 0x3F0 + d);
    }
    add(0, 0x2AD);
    add(0, 0xF288);
    add(0, 0xF289);
    add(3, 0x3FE);
    let mut atlas = vec![0u8; keys.len() * 256];
    for (i, &(layer, e)) in keys.iter().enumerate() {
        let chars = if layer == 0 { &bar_chars } else { &panel_chars };
        let tile = bg_tile(chars, &pal, e);
        for y in 0..8 {
            atlas[(y * keys.len() * 8 + i * 8) * 4..][..32]
                .copy_from_slice(&tile[y * 32..y * 32 + 32]);
        }
    }

    // The panel's sprites (Mario's face, the "L" hint) as an overlay.
    let mut sprites = Rgba {
        width: 240,
        height: 160,
        pixels: vec![0; 240 * 160 * 4],
    };
    draw_oam(m, &mut sprites, 0..128)?;

    MapScreenAsset {
        map: view,
        bar,
        bar_scroll,
        panel,
        panel_scroll,
        keys,
        tiles: Rgba {
            width: 8 * (atlas.len() / 256) as u32,
            height: 8,
            pixels: atlas,
        },
        sprites,
    }
    .write(out_dir, world)
    .map_err(|e| ExtractError(e.to_string()))?;
    println!("  world map {}", world + 1);
    Ok(())
}

/// After game over (mode 1 sub-state 0x2C, 0x08012458): the map again, dimmed, with the
/// "GAME OVER / CONTINUE / QUIT" panel; the menu cursor (0x08012E6C) is OAM slot 72.
const FN_GAME_OVER_MENU: u32 = 0x0801_2458;
const FN_MENU_CURSOR: u32 = 0x0801_2E6C;
const MENU_CURSOR: u32 = 0x0300_37CC;
const LIVES: u32 = 0x0300_2A6A;
const PLAYERS: u32 = 0x0300_385E;
const CURSOR_SLOT: u32 = 72;

/// The menu screen (map + panel, no status bar; with the panel's Mario head) and the
/// cursor sprite.
pub fn extract_game_over_menu(
    m: &mut Machine,
    out_dir: &std::path::Path,
) -> Result<(), ExtractError> {
    setup(m)?;
    m.w8(PLAYERS, 1)?;
    m.w16(LIVES, 0xFFFF)?;
    m.call(FN_GAME_OVER_MENU, &[])?;
    m.w8(MENU_CURSOR, 0x58)?;
    m.call(FN_MENU_CURSOR, &[])?;
    let pal = parse_palette(&m.read(BG_PAL_BUFFER, 0x200)?);
    let mut screen = Rgba {
        width: 240,
        height: 160,
        pixels: vec![0; 240 * 160 * 4],
    };
    // Backdrop, then BG1 (map, priority 3) and BG3 (panel, priority 1).
    for px in screen.pixels.as_chunks_mut::<4>().0 {
        *px = pal[0].to_rgba(255);
    }
    for bg in [1, 3] {
        let (map, chars, scroll) = layer(m, bg)?;
        let view = render_view(&map, layer_width(m, bg)?, &chars, &pal, scroll);
        for (dst, src) in screen
            .pixels
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(view.pixels.as_chunks::<4>().0)
        {
            if src[3] != 0 {
                *dst = *src;
            }
        }
    }
    // The panel's sprites except the cursor (drawn on its own, it blinks).
    let cursor = read_u16s(m, OAM_BUF + CURSOR_SLOT * 8, 3)?;
    m.w16(OAM_BUF + CURSOR_SLOT * 8, 0xA0)?;
    draw_oam(m, &mut screen, 0..128)?;
    m.w16(OAM_BUF + CURSOR_SLOT * 8, cursor[0])?;
    let mut only_cursor = Rgba {
        width: 240,
        height: 160,
        pixels: vec![0; 240 * 160 * 4],
    };
    draw_oam(m, &mut only_cursor, CURSOR_SLOT..CURSOR_SLOT + 1)?;
    let (cx, cy) = ((cursor[1] & 0x1FF) as u32, (cursor[0] & 0xFF) as u32);
    let mut cell = vec![0u8; 8 * 8 * 4];
    for y in 0..8 {
        let o = (((cy + y) * 240 + cx) * 4) as usize;
        cell[(y * 32) as usize..][..32].copy_from_slice(&only_cursor.pixels[o..o + 32]);
    }
    screen
        .write_png(&game_over_menu_path(out_dir))
        .map_err(|e| ExtractError(e.to_string()))?;
    Rgba {
        width: 8,
        height: 8,
        pixels: cell,
    }
    .write_png(&game_over_cursor_path(out_dir))
    .map_err(|e| ExtractError(e.to_string()))?;
    println!("  game over menu (cursor x {cx}, y {cy})");
    Ok(())
}

/// Game-over pieces: 10 letter cells (8x16, slots 1-10 = tiles 0x100-0x109) then the
/// Mario head (16x16), in one 96x16 strip.
pub fn extract_game_over(
    rom: &[u8],
    m: &mut Machine,
    out_dir: &std::path::Path,
) -> Result<(), ExtractError> {
    setup(m)?;
    m.call(FN_GAME_OVER_INIT, &[])?;
    let chars = m.read(OBJ_VRAM, 0x8000)?;
    let pal = parse_palette(&m.read(OBJ_PAL_BUFFER, 0x200)?);
    let order = {
        let p = u32::from_le_bytes(rom[0x0013_5E7C..0x0013_5E80].try_into().unwrap()) - 0x0800_0000;
        rom[p as usize..p as usize + 10].to_vec()
    };
    let mut px = vec![0u8; 96 * 16 * 4];
    let mut put = |tile: u32, ox: u32, w: u32, pal_no: u32| {
        for sy in 0..16u32 {
            for sx in 0..w {
                let t = tile + (sy / 8) * 32 + sx / 8;
                let d = &chars[(t * 32) as usize..][..32];
                let b = d[((sy % 8) * 4 + (sx % 8) / 2) as usize];
                let c = if sx & 1 == 1 { b >> 4 } else { b & 0xF };
                if c != 0 {
                    px[((sy * 96 + ox + sx) * 4) as usize..][..4]
                        .copy_from_slice(&color(&pal, pal_no, c));
                }
            }
        }
    };
    // OAM slot s (1-10) gets letter index 10 - s: slot 10 is drawn first with index 0.
    for slot in 1..=10u32 {
        let index = (10 - slot) as usize;
        put(
            GAME_OVER_LETTERS | order[index] as u32,
            (slot - 1) * 8,
            8,
            1,
        );
    }
    put(GAME_OVER_HEAD, 80, 16, 7);
    Rgba {
        width: 96,
        height: 16,
        pixels: px,
    }
    .write_png(&game_over_path(out_dir))
    .map_err(|e| ExtractError(e.to_string()))?;
    println!("  game over");
    Ok(())
}

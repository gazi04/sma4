//! The spade panel's bonus game (`spade/`, see `sma4_assets::SpadeAsset`): Toad's room as
//! level 0x240's loader 0x08015F10 builds it, his message box and messages, and the slots as
//! 0x0803377C loads them for each streak count, all read back from the emulated machine;
//! the win's BG2 tables come straight from the ROM.

use sma4_assets::{
    Rgba, SPADE_COUNTS, SPADE_FRAME, SPADE_HOST, SPADE_HOST_BOX, SPADE_HOST_TOAD, SPADE_MESSAGES, SPADE_TILES,
    SlotWin, SpadeAsset, spade_host_text, spade_path, spade_prize, spade_reels,
};
use sma4_rom::Rom;
use sma4_rom::gfx::{Bgr555, parse_palette};

use crate::emu::Machine;
use crate::level::ExtractError;
use crate::screens::{bg_tile, draw_oam, layer, layer_width, render_view};

const PLAYER_PTR: u32 = 0x0300_7818;
const PLAYER_GFX_PTR: u32 = 0x0300_7820;
const MAP_STATE_PTR: u32 = 0x0300_7824;
const OBJ_ANIM_PTR: u32 = 0x0300_7828;
const OBJ_BASE_PTR: u32 = 0x0300_7848;
const OAM_BUF: u32 = 0x0300_2DC8;
/// Next free OAM slot (0x08033298 counts it up).
const OAM_NEXT: u32 = 0x0300_3781;
const OAM_FIRST: u8 = 0x40;
const BG_PAL_BUFFER: u32 = 0x0200_0000;
const VRAM: u32 = 0x0600_0000;
const BG_CNT: u32 = 0x0300_2DA4;
const BG3_SCROLL: u32 = 0x0300_3760;

/// Game kind 0x030038EC (1: the slots), tileset 0x03003849 (15: the room, 0x10: the slots),
/// graphics set 0x030037DF, "loaded" flag 0x030038A9 (0x0803377C), streak 0x03002CD8.
const KIND: u32 = 0x0300_38EC;
const TILESET: u32 = 0x0300_3849;
const GFX_SET: u32 = 0x0300_37DF;
const LOADED: u32 = 0x0300_38A9;
const STREAK: u32 = 0x0300_2CD8;
/// The room's state block (0x03003904 -> 0x18 bytes from the heap): [0] state, [5] box row.
const STATE_PTR: u32 = 0x0300_3904;
const STATE_BUF: u32 = 0x0203_D000;
/// Level 0x240 (0x08135B20[0]) and its pointer (0x08136200, as 0x08009790 sets it).
const ROOM_LEVEL: u32 = 0x240;
const LEVEL_PTRS: u32 = 0x0813_6200;
const LEVEL_ID: [u32; 2] = [0x0300_3B08, 0x0300_3B0A];
const LEVEL_PTR: [u32; 2] = [0x0300_3B04, 0x0300_3B0C];

const FN_LEVEL_INIT: u32 = 0x0801_6434;
const FN_SLOTS_LOAD: u32 = 0x0803_377C;
const FN_TOAD: u32 = 0x0803_3418;
const FN_BOX: u32 = 0x0803_2FC8;
const FN_TEXT: u32 = 0x0803_30B8;
const FN_DRAW_LIST: u32 = 0x0803_3298;
/// Room state after the message (0x081440C8[8]).
const ROOM_IDLE: u8 = 8;
const BOX_ROWS: u32 = 8;

/// Toad's messages: 0x08244318[language] -> pointers.
const TEXT_TABLES: u32 = 0x0824_4318;
const LANG: u32 = 0;

/// Win tables by prize (0x08033E54, 0x08034578, 0x08034650).
const SIGN_AT: u32 = 0x0814_3E74;
const SIGN: u32 = 0x0814_3EA0;
const SPARKLE_AT: u32 = 0x0814_3F3C;
const SPARKLES: u32 = 0x0814_4034;
const END_AT: u32 = 0x0814_4060;
const CYCLE: u32 = 0x0814_408C;
const PRIZE_SPRITES: u32 = 0x0814_409C;
/// Every prize 0x08143E14 holds.
const PRIZES: [u8; 5] = [2, 3, 5, 7, 10];
/// The prize sprite's x (0x0803446C) and a y to draw it at.
const PRIZE_X: u32 = 0x78;
const PRIZE_Y: u32 = 0x40;
const BLANK: u16 = 0xFD;

fn err(e: sma4_assets::AssetError) -> ExtractError {
    ExtractError(e.to_string())
}

/// Mode-1 state as the map leaves it for the spade (0x08009790), then the level loader:
/// Toad's room.
fn setup_room(m: &mut Machine, rom: &Rom) -> Result<(), ExtractError> {
    m.clear_ram()?;
    for (ptr, val) in [
        (PLAYER_PTR, 0x0203_A000),
        (PLAYER_GFX_PTR, 0x0203_B000),
        (MAP_STATE_PTR, 0x0203_C000),
        (OBJ_BASE_PTR, 0x0203_8000),
        (OBJ_ANIM_PTR, 0x0203_9000),
        (STATE_PTR, STATE_BUF),
    ] {
        m.w32(ptr, val)?;
    }
    clear_oam(m)?;
    m.w8(KIND, 1)?;
    m.w8(TILESET, 0x0F)?;
    let level = rom.ptr(LEVEL_PTRS + ROOM_LEVEL * 4)?;
    for a in LEVEL_ID {
        m.w16(a, ROOM_LEVEL as u16)?;
    }
    for a in LEVEL_PTR {
        m.w32(a, level)?;
    }
    m.call(FN_LEVEL_INIT, &[])?;
    Ok(())
}

/// Sub-state 0x26 for streak count `count`.
fn load_slots(m: &mut Machine, count: u8) -> Result<(), ExtractError> {
    m.w8(STREAK, count << 2)?;
    m.w8(LOADED, 1)?;
    m.w8(TILESET, 0x10)?;
    m.w8(GFX_SET, 0x10)?;
    m.call(FN_SLOTS_LOAD, &[])?;
    Ok(())
}

fn clear_oam(m: &mut Machine) -> Result<(), ExtractError> {
    for i in 0..128 {
        m.w16(OAM_BUF + i * 8, 0xA0)?;
    }
    m.w8(OAM_NEXT, OAM_FIRST)?;
    Ok(())
}

fn screen() -> Rgba {
    Rgba { width: 240, height: 160, pixels: vec![0; 240 * 160 * 4] }
}

/// `top` over `bottom` (both 240x160, transparent = alpha 0).
fn over(bottom: &mut Rgba, top: &Rgba) {
    for (b, t) in bottom.pixels.chunks_mut(4).zip(top.pixels.chunks(4)) {
        if t[3] != 0 {
            b.copy_from_slice(t);
        }
    }
}

fn view(m: &Machine, bg: u32) -> Result<Rgba, ExtractError> {
    let pal = parse_palette(&m.read(BG_PAL_BUFFER, 0x200)?);
    let (map, chars, scroll) = layer(m, bg)?;
    Ok(render_view(&map, layer_width(m, bg)?, &chars, &pal, scroll))
}

fn backdrop(m: &Machine) -> Result<[u8; 4], ExtractError> {
    let pal = parse_palette(&m.read(BG_PAL_BUFFER, 2)?);
    Ok(pal[0].to_rgba(255))
}

/// BG2 and BG1 over the backdrop.
fn room(m: &Machine) -> Result<Rgba, ExtractError> {
    let mut out = screen();
    let b = backdrop(m)?;
    for p in out.pixels.chunks_mut(4) {
        p.copy_from_slice(&b);
    }
    over(&mut out, &view(m, 2)?);
    over(&mut out, &view(m, 1)?);
    Ok(out)
}

fn message(rom: &Rom, k: u32) -> Result<Vec<u8>, ExtractError> {
    let mut p = rom.ptr(rom.ptr(TEXT_TABLES + LANG * 4)? + k * 4)?;
    let mut bytes = Vec::new();
    loop {
        let b = rom.u8(p)?;
        bytes.push(b);
        p += 1;
        if b == 0xFF {
            return Ok(bytes);
        }
    }
}

/// A u16 list ending in 0xFFFF.
fn list(rom: &Rom, mut p: u32) -> Result<Vec<u16>, ExtractError> {
    let mut out = Vec::new();
    loop {
        let v = rom.u16(p)?;
        if v == 0xFFFF {
            return Ok(out);
        }
        out.push(v);
        p += 2;
    }
}

fn wins(rom: &Rom) -> Result<(Vec<SlotWin>, [Vec<u16>; 4]), ExtractError> {
    let mut wins = Vec::new();
    for p in PRIZES {
        let p32 = p as u32;
        let signs = rom.ptr(SIGN_AT + p32 * 4)?;
        let sparkles = rom.ptr(SPARKLES + p32 * 4)?;
        let mut w = SlotWin {
            prize: p,
            sparkle_at: [list(rom, rom.ptr(SPARKLE_AT + p32 * 8)?)?, list(rom, rom.ptr(SPARKLE_AT + p32 * 8 + 4)?)?],
            end_at: list(rom, rom.ptr(END_AT + p32 * 4)?)?,
            ..Default::default()
        };
        for (i, v) in w.sign_at.iter_mut().enumerate() {
            *v = rom.u16(signs + i as u32 * 2)?;
        }
        for (i, v) in w.sign.iter_mut().enumerate() {
            *v = rom.u16(SIGN + (p32 * 4 + i as u32) * 2)?;
        }
        for (i, v) in w.sparkles.iter_mut().flatten().enumerate() {
            *v = rom.u16(sparkles + i as u32 * 2)?;
        }
        wins.push(w);
    }
    let mut cycle: [Vec<u16>; 4] = Default::default();
    for (i, c) in cycle.iter_mut().enumerate() {
        *c = list(rom, rom.ptr(CYCLE + i as u32 * 4)?)?;
    }
    Ok((wins, cycle))
}

/// BG2's first 32x32 screen block.
fn bg2_map(m: &Machine) -> Result<Vec<u16>, ExtractError> {
    let c = m.read(BG_CNT + 4, 2)?;
    let cnt = u16::from_le_bytes([c[0], c[1]]);
    let base = VRAM + ((cnt as u32 >> 8) & 0x1F) * 0x800;
    Ok(m.read(base, 0x800)?.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect())
}

pub fn extract(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    let png = |img: &Rgba, name: &str| img.write_png(&spade_path(out_dir, name)).map_err(err);
    setup_room(m, rom)?;
    png(&room(m)?, SPADE_HOST)?;
    clear_oam(m)?;
    m.call(FN_TOAD, &[])?;
    let mut toad = screen();
    draw_oam(m, &mut toad, 0..128)?;
    png(&toad, SPADE_HOST_TOAD)?;
    let s = m.read(BG3_SCROLL, 6)?;
    let host_scroll = (u16::from_le_bytes([s[0], s[1]]), u16::from_le_bytes([s[4], s[5]]));

    let mut messages: [Vec<u8>; SPADE_MESSAGES] = Default::default();
    for (k, msg) in messages.iter_mut().enumerate() {
        // State 0 draws the box (and starts message min(count, 3)), state 7 prints it.
        m.w8(STREAK, (k as u8) << 2)?;
        m.w8(STATE_BUF, 0)?;
        m.w8(STATE_BUF + 5, 0)?;
        for _ in 0..BOX_ROWS {
            m.call(FN_BOX, &[])?;
        }
        if k == 0 {
            png(&view(m, 3)?, SPADE_HOST_BOX)?;
        }
        for _ in 0..0x1000 {
            if m.read(STATE_BUF, 1)?[0] >= ROOM_IDLE {
                break;
            }
            m.call(FN_TEXT, &[])?;
        }
        png(&view(m, 3)?, &spade_host_text(k))?;
        *msg = message(rom, k as u32)?;
    }

    let (wins, cycle) = wins(rom)?;
    let mut bg2 = Vec::new();
    let mut tiles = None;
    let mut back = [0; 4];
    for c in 0..SPADE_COUNTS {
        load_slots(m, c as u8)?;
        let pal = parse_palette(&m.read(BG_PAL_BUFFER, 0x200)?);
        // BG1: the whole layer (its rows scroll on their own).
        let (map, chars, _) = layer(m, 1)?;
        png(&whole_layer(&map, layer_width(m, 1)?, &chars, &pal), &spade_reels(c))?;
        bg2.push(bg2_map(m)?);
        if c == 0 {
            png(&view(m, 3)?, SPADE_FRAME)?;
            back = backdrop(m)?;
            tiles = Some((layer(m, 2)?.1, pal));
        }
    }
    // BG2's tiles: every entry its maps and the win can show.
    let mut keys: Vec<u16> = Vec::new();
    let win_entries = wins.iter().flat_map(|w| w.sign.iter().chain(w.sparkles.iter().flatten()).copied());
    for e in bg2.iter().flatten().copied().chain(win_entries).chain([BLANK]) {
        if !keys.contains(&e) {
            keys.push(e);
        }
    }
    if let Some((chars, pal)) = &tiles {
        let n = keys.len();
        let mut atlas = Rgba { width: 8 * n as u32, height: 8, pixels: vec![0; n * 256] };
        for (i, &e) in keys.iter().enumerate() {
            let t = bg_tile(chars, pal, e);
            for y in 0..8 {
                atlas.pixels[(y * n * 8 + i * 8) * 4..][..32].copy_from_slice(&t[y * 32..y * 32 + 32]);
            }
        }
        png(&atlas, SPADE_TILES)?;
    }

    // The prize sprites (0x08033298 with 0x0814409C[prize]).
    let mut prizes = Vec::new();
    for p in PRIZES {
        clear_oam(m)?;
        let list = rom.ptr(PRIZE_SPRITES + p as u32 * 4)?;
        m.call(FN_DRAW_LIST, &[list, PRIZE_X, PRIZE_Y])?;
        let mut img = screen();
        draw_oam(m, &mut img, 0..128)?;
        let (x0, y0, x1, y1) = bounds(&img);
        prizes.push((p, x0 - PRIZE_X as i32, y0 - PRIZE_Y as i32));
        png(&crop(&img, x0, y0, x1, y1), &spade_prize(p))?;
    }

    SpadeAsset { messages, host_scroll, bg2, wins, cycle, backdrop: back, tile_keys: keys, prizes }
        .write(out_dir)
        .map_err(err)?;
    println!("  spade game: Toad's room, {SPADE_COUNTS} slot layouts");
    Ok(())
}

/// A whole BG layer (no scroll).
fn whole_layer(map: &[u16], width: usize, chars: &[u8], pal: &[Bgr555]) -> Rgba {
    let height = map.len() / width;
    let (w, h) = (width * 8, height * 8);
    let mut px = vec![0u8; w * h * 4];
    for (i, &e) in map.iter().enumerate() {
        let (tx, ty) = (i % width, i / width);
        let t = bg_tile(chars, pal, e);
        for y in 0..8 {
            px[((ty * 8 + y) * w + tx * 8) * 4..][..32].copy_from_slice(&t[y * 32..y * 32 + 32]);
        }
    }
    Rgba { width: w as u32, height: h as u32, pixels: px }
}

/// The opaque pixels' bounding box.
fn bounds(img: &Rgba) -> (i32, i32, i32, i32) {
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, 0, 0);
    for y in 0..img.height as i32 {
        for x in 0..img.width as i32 {
            if img.pixels[((y * img.width as i32 + x) * 4 + 3) as usize] != 0 {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
        }
    }
    if x0 > x1 { (0, 0, 1, 1) } else { (x0, y0, x1, y1) }
}

fn crop(img: &Rgba, x0: i32, y0: i32, x1: i32, y1: i32) -> Rgba {
    let (w, h) = ((x1 - x0) as u32, (y1 - y0) as u32);
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y1 {
        let o = ((y * img.width as i32 + x0) * 4) as usize;
        pixels.extend_from_slice(&img.pixels[o..o + (w * 4) as usize]);
    }
    Rgba { width: w, height: h, pixels }
}

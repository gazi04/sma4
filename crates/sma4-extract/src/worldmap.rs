//! The interactive world map (mode 1 sub-states 0x10-0x15; see docs/re-notes.md, "World
//! map walking"): runs the map init in the emulator, then reads back the tile grid, the
//! enterable spots, the metatile pictures (with the BG tile animation) and Mario's map
//! sprite.

use sma4_assets::{MAP_ANIM_FRAMES, MAP_MARIO_FORMS, MAP_MARIO_H, MAP_MARIO_W, MapNode, Rgba, WorldMapAsset};
use sma4_rom::{Rom, gfx::parse_palette};

use crate::emu::Machine;
use crate::level::ExtractError;
use crate::screens::{bg_tile, draw_oam, layer, layer_width, render_view};

const PLAYER_PTR: u32 = 0x0300_7818;
const PLAYER_GFX_PTR: u32 = 0x0300_7820;
const MAP_STATE_PTR: u32 = 0x0300_7824;
const OBJ_BASE_PTR: u32 = 0x0300_7848;
const MAP_STATE: u32 = 0x0203_C000;
const WORLD: u32 = 0x0300_2A69;
const LANGUAGE: u32 = 0x0300_3777;
const OAM_BUF: u32 = 0x0300_2DC8;
const BG_PAL_BUFFER: u32 = 0x0200_0000;
const VRAM: u32 = 0x0600_0000;
const BG1_CNT: u32 = 0x0300_2DA6;
/// Map init (mode 1 sub-state 0x10).
const FN_MAP_INIT: u32 = 0x0800_BE48;
/// Saved map position the init copies into the map state (+0 y, +2 screen, +4 x).
const SAVED_Y: u32 = 0x0300_37A9;
const SAVED_X: u32 = 0x0300_37AD;
/// Where a world starts: the START tile (Y 0x40, X 0x20).
// TODO(re): the new-game routine that stores this was not traced; SMB3 uses the same spot.
const START: (u8, u16) = (0x40, 0x20);
/// Map tile RAM per screen: 0x08136134[screen] + 0x100, 16 columns, rows from y 0x10.
const SCREEN_PTRS: u32 = 0x0813_6134;
/// 0x08009428 copies 12 rows of 16 tiles per screen (0x08135AAC[world]).
const ROWS: usize = 12;
const COLS: usize = 16;
/// Map level tables (0x08009A3C), one pointer per world.
const NODE_START: u32 = 0x0813_3A64;
const NODE_ROWS: u32 = 0x0813_3A88;
const NODE_COLS: u32 = 0x0813_3AAC;
const NODE_ENEMIES: u32 = 0x0813_3AD0;
const NODE_LEVELS: u32 = 0x0813_3AF4;
/// Enterable thresholds 0x030037D6[id >> 6], set up by the map init.
const THRESHOLDS: u32 = 0x0300_37D6;
/// Map tileset 0x03003849 picks the metatile table 0x08135C0C[tileset]: four u16 BG
/// entries per id (0x08012118: +0 top-left, +1 bottom-left, +2 top-right, +3 bottom-right).
const MAP_TILESET: u32 = 0x0300_3849;
const METATILE_TABLES: u32 = 0x0813_5C0C;
/// Map tile animation: the map VBlank (0x080013E0) copies 0x1000 bytes (BG tiles 0x000-0x07F)
/// from 0x0201E000 + frame * 0x1000 when 0x03003B5C bit 7 is clear; 0x0800ADFC advances
/// the frame (0-3) when its timer runs out and reloads the timer from
/// 0x08135C5E[world * 4 + frame]. 0x0800AFC4 fills a frame from LZ77 0x08135C84[frame]
/// at offset 0x08135CA4[frame] (worlds 4 and 7 use entries 4-7).
const ANIM_LZ: u32 = 0x0813_5C84;
const ANIM_OFFSETS: u32 = 0x0813_5CA4;
const ANIM_TIMES: u32 = 0x0813_5C5E;
const ANIM_BYTES: usize = 0x1000;
/// Mario's map sprite (0x0800C45C): OAM slots 68 (upper half, big forms) and 69 (lower);
/// pose from the map form 0x03003CB6 and frame-counter bit 3.
const FN_DRAW_MARIO: u32 = 0x0800_C45C;
const MAP_FORM: u32 = 0x0300_3CB6;
const FRAME_COUNTER: u32 = 0x0300_3AE8;
const MAP_SUB_STATE: u32 = 0x0300_385D;
const IDLE: u8 = 0x0E;
const MARIO_SLOTS: std::ops::Range<u32> = 68..70;

fn setup(m: &mut Machine, world: u8) -> Result<(), ExtractError> {
    m.clear_ram()?;
    for (ptr, val) in
        [(PLAYER_PTR, 0x0203_A000), (PLAYER_GFX_PTR, 0x0203_B000), (MAP_STATE_PTR, MAP_STATE), (OBJ_BASE_PTR, 0x0203_8000)]
    {
        m.w32(ptr, val)?;
    }
    m.w8(LANGUAGE, 0)?;
    m.w8(WORLD, world)?;
    m.w8(SAVED_Y, START.0)?;
    m.w8(SAVED_X, START.1 as u8)?;
    for i in 0..128 {
        m.w16(OAM_BUF + i * 8, 0xA0)?;
    }
    m.call(FN_MAP_INIT, &[])?;
    Ok(())
}

/// The world's entries of the map level tables.
pub fn nodes(rom: &Rom, world: u8) -> Result<Vec<MapNode>, ExtractError> {
    let w = world as u32 * 4;
    let (rows, cols) = (rom.ptr(NODE_ROWS + w)?, rom.ptr(NODE_COLS + w)?);
    let (enemies, levels) = (rom.ptr(NODE_ENEMIES + w)?, rom.ptr(NODE_LEVELS + w)?);
    // The per-screen start indices end the list: the last screen's index is the count.
    let starts = rom.ptr(NODE_START + w)?;
    let count = (0..4).map(|s| rom.u8(starts + s).unwrap_or(0)).max().unwrap_or(0) as u32;
    (0..count)
        .map(|i| {
            let (r, c) = (rom.u8(rows + i)?, rom.u8(cols + i)?);
            Ok(MapNode {
                y: r & 0xF0,
                x: ((c as u16 >> 4) << 8) | ((c as u16 & 0xF) << 4),
                tileset: r & 0xF,
                level: rom.u16(levels + i * 2)?,
                enemies: rom.u16(enemies + i * 2)?,
            })
        })
        .collect()
}

fn read_grid(m: &Machine, rom: &Rom, screens: usize) -> Result<Vec<u8>, ExtractError> {
    let mut grid = vec![0u8; COLS * screens * ROWS];
    for s in 0..screens {
        let base = rom.u32(SCREEN_PTRS + s as u32 * 4)? + 0x100;
        let ram = m.read(base, ROWS * COLS)?;
        for r in 0..ROWS {
            for c in 0..COLS {
                grid[r * COLS * screens + s * COLS + c] = ram[r * COLS + c];
            }
        }
    }
    Ok(grid)
}

/// One metatile (16x16 RGBA) from its four BG entries.
fn metatile(chars: &[u8], pal: &[sma4_rom::gfx::Bgr555], entries: [u16; 4]) -> Vec<u8> {
    let mut px = vec![0u8; 16 * 16 * 4];
    // Column-major: top-left, bottom-left, top-right, bottom-right.
    for (k, &e) in entries.iter().enumerate() {
        let (ox, oy) = ((k / 2) * 8, (k % 2) * 8);
        let t = bg_tile(chars, pal, e);
        for y in 0..8 {
            px[((oy + y) * 16 + ox) * 4..][..32].copy_from_slice(&t[y * 32..y * 32 + 32]);
        }
    }
    px
}

/// Draws `grid` (cols x rows) with metatile pictures `cells` at `origin` onto a 240x160
/// screen, to line the grid up with the ROM's own BG view.
fn render_grid(grid: &[u8], cols: usize, cells: &[Vec<u8>], origin: (i32, i32)) -> Vec<u8> {
    let mut px = vec![0u8; 240 * 160 * 4];
    for (i, &id) in grid.iter().enumerate() {
        let (c, r) = ((i % cols) as i32, (i / cols) as i32);
        let cell = &cells[id as usize];
        for y in 0..16 {
            for x in 0..16 {
                let (sx, sy) = (origin.0 + c * 16 + x, origin.1 + r * 16 + y);
                if (0..240).contains(&sx) && (0..160).contains(&sy) {
                    let s = ((y * 16 + x) * 4) as usize;
                    px[((sy * 240 + sx) * 4) as usize..][..4].copy_from_slice(&cell[s..s + 4]);
                }
            }
        }
    }
    px
}

pub fn extract_world_map(rom: &Rom, m: &mut Machine, world: u8, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    setup(m, world)?;
    let nodes = nodes(rom, world)?;
    let screens = nodes.iter().map(|n| (n.x >> 8) as usize + 1).max().unwrap_or(1);
    let grid = read_grid(m, rom, screens)?;
    let cols = COLS * screens;
    let t = m.read(THRESHOLDS, 4)?;
    let thresholds = [t[0], t[1], t[2], t[3]];

    let pal = parse_palette(&m.read(BG_PAL_BUFFER, 0x200)?);
    let cnt = u16::from_le_bytes(m.read(BG1_CNT, 2)?.try_into().unwrap_or([0, 0]));
    let char_base = VRAM + ((cnt as u32 >> 2) & 3) * 0x4000;
    let table = rom.ptr(METATILE_TABLES + m.read(MAP_TILESET, 1)?[0] as u32 * 4)?;
    let entries: Vec<[u16; 4]> = (0..256u32)
        .map(|id| -> Result<[u16; 4], ExtractError> {
            let e = |k: u32| rom.u16(table + (id * 4 + k) * 2);
            Ok([e(0)?, e(1)?, e(2)?, e(3)?])
        })
        .collect::<Result<_, _>>()?;

    // Atlas: one 16-px column per animation frame, 256 metatiles down.
    let (aw, ah) = (16 * MAP_ANIM_FRAMES, 16 * 256);
    let mut atlas = vec![0u8; (aw * ah * 4) as usize];
    let mut frame0 = Vec::new();
    for f in 0..MAP_ANIM_FRAMES {
        let mut chars = m.read(char_base, 0x8000)?;
        let k = f + if world == 3 || world == 6 { 4 } else { 0 };
        let frame = rom.decompress(rom.ptr(ANIM_LZ + k * 4)?)?;
        let off = rom.u16(ANIM_OFFSETS + k * 2)? as usize;
        chars[..ANIM_BYTES].copy_from_slice(&frame[off..off + ANIM_BYTES]);
        let cells: Vec<Vec<u8>> = entries.iter().map(|&e| metatile(&chars, &pal, e)).collect();
        for (id, cell) in cells.iter().enumerate() {
            for y in 0..16 {
                let dst = (((id as u32 * 16 + y) * aw + f * 16) * 4) as usize;
                atlas[dst..dst + 64].copy_from_slice(&cell[(y * 64) as usize..(y * 64 + 64) as usize]);
            }
        }
        if f == 0 {
            frame0 = cells;
        }
    }

    // Line the grid up with the ROM's BG1 view (frame 0 of the animation is in VRAM).
    let (bg1, bg1_chars, scroll) = layer(m, 1)?;
    let view = render_view(&bg1, layer_width(m, 1)?, &bg1_chars, &pal, scroll);
    let origin = (-16..=16)
        .flat_map(|oy| (-16..=16).map(move |ox| (ox, oy)))
        .max_by_key(|&o| {
            let ours = render_grid(&grid, cols, &frame0, o);
            ours.as_chunks::<4>().0.iter().zip(view.pixels.as_chunks::<4>().0).filter(|(a, b)| a == b).count()
        })
        .unwrap_or((0, 0));
    let ours = render_grid(&grid, cols, &frame0, origin);
    let same = ours.as_chunks::<4>().0.iter().zip(view.pixels.as_chunks::<4>().0).filter(|(a, b)| a == b).count();
    println!("  world map {} walk: grid {cols}x{ROWS}, origin {origin:?}, {same}/{} px as the ROM's view", world + 1, 240 * 160);

    // Mario's map sprite per form and animation phase.
    let mut mario = Rgba {
        width: MAP_MARIO_W * MAP_MARIO_FORMS * 2,
        height: MAP_MARIO_H,
        pixels: vec![0; (MAP_MARIO_W * MAP_MARIO_FORMS * 2 * MAP_MARIO_H * 4) as usize],
    };
    m.w8(MAP_SUB_STATE, IDLE)?;
    for (i, form) in [0u8, 1, 3].into_iter().enumerate() {
        for phase in 0..2u32 {
            for s in 0..128 {
                m.w16(OAM_BUF + s * 8, 0xA0)?;
            }
            m.w8(MAP_FORM, form)?;
            m.w8(FRAME_COUNTER, (phase * 8) as u8)?;
            m.call(FN_DRAW_MARIO, &[])?;
            let lower = m.read(OAM_BUF + 69 * 8, 4)?;
            let (ly, lx) = (lower[0] as i32, (u16::from_le_bytes([lower[2], lower[3]]) & 0x1FF) as i32);
            let mut canvas = Rgba { width: 240, height: 160, pixels: vec![0; 240 * 160 * 4] };
            draw_oam(m, &mut canvas, MARIO_SLOTS)?;
            let cell = (i as u32 * 2 + phase) * MAP_MARIO_W;
            for y in 0..MAP_MARIO_H as i32 {
                for x in 0..MAP_MARIO_W as i32 {
                    let (sx, sy) = (lx + x, ly - 16 + y);
                    if !(0..240).contains(&sx) || !(0..160).contains(&sy) {
                        continue;
                    }
                    let s = ((sy * 240 + sx) * 4) as usize;
                    let d = ((y as u32 * mario.width + cell + x as u32) * 4) as usize;
                    mario.pixels[d..d + 4].copy_from_slice(&canvas.pixels[s..s + 4]);
                }
            }
        }
    }

    let anim_ticks: Vec<u8> =
        (0..MAP_ANIM_FRAMES).map(|f| rom.u8(ANIM_TIMES + world as u32 * 4 + f).map(|t| t + 1)).collect::<Result<_, _>>()?;
    WorldMapAsset {
        anim_ticks: [anim_ticks[0], anim_ticks[1], anim_ticks[2], anim_ticks[3]],
        cols,
        rows: ROWS,
        grid,
        start: START,
        thresholds,
        nodes,
        origin,
        atlas: Rgba { width: aw, height: ah, pixels: atlas },
        mario,
    }
    .write(out_dir, world)
    .map_err(|e| ExtractError(e.to_string()))
}

/// Tiles A always enters on (0x0801112C checks 0x0813A616 before the thresholds).
const ENTER_ALWAYS: u32 = 0x0813_A616;
const ENTER_ALWAYS_LEN: u32 = 11;
/// The King's castle: its "level" starts the airship sequence, not ported yet.
const KINGS_CASTLE: u8 = 0xC9;

/// The levels behind the world's enterable spots (course levels only: Toad houses and the
/// spade game, whose ids have bit 15 set or tileset 15, are not levels, and the King's
/// castle leads to the airship), first-seen order.
pub fn course_levels(rom: &Rom, m: &mut Machine, world: u8) -> Result<Vec<(u16, u8, u16)>, ExtractError> {
    setup(m, world)?;
    let nodes = nodes(rom, world)?;
    let screens = nodes.iter().map(|n| (n.x >> 8) as usize + 1).max().unwrap_or(1);
    let grid = read_grid(m, rom, screens)?;
    let t = m.read(THRESHOLDS, 4)?;
    let always: Vec<u8> = (0..ENTER_ALWAYS_LEN).map(|i| rom.u8(ENTER_ALWAYS + i)).collect::<Result<_, _>>()?;
    let mut out: Vec<(u16, u8, u16)> = Vec::new();
    for n in nodes {
        let (row, col) = (((n.y as usize).wrapping_sub(0x10) >> 4) & 0xF, ((n.x >> 8) as usize) * COLS + (n.x as usize & 0xFF) / 16);
        let Some(&tile) = grid.get(row * COLS * screens + col) else { continue };
        let enterable = always.contains(&tile) || tile >= t[(tile >> 6) as usize];
        let course = n.level & 0x8000 == 0 && n.enemies & 0x8000 == 0 && n.tileset != 15 && tile != KINGS_CASTLE;
        if enterable && course && !out.iter().any(|l| l.0 == n.level) {
            out.push((n.level, n.tileset, n.enemies));
        }
    }
    Ok(out)
}

//! The interactive world map (mode 1 sub-states 0x10-0x15; see docs/re-notes.md, "World
//! map walking"): runs the map init in the emulator, then reads back the tile grid, the
//! enterable spots, the metatile pictures (with the BG tile animation) and Mario's map
//! sprite.

use sma4_assets::{
    MAP_ANIM_FRAMES, MAP_MARIO_FORMS, MAP_MARIO_H, MAP_MARIO_W, MAP_SPIN_PHASES, MAP_SPIN_SIZE,
    MapNode, Rgba, WorldMapAsset,
};
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
const BG0_CNT: u32 = 0x0300_2DA4;
/// Map init (mode 1 sub-state 0x10).
const FN_MAP_INIT: u32 = 0x0800_BE48;
/// Saved map position the init copies into the map state (+0 y, +2 screen, +4 x).
const SAVED_Y: u32 = 0x0300_37A9;
const SAVED_X: u32 = 0x0300_37AD;
/// Where a world starts (0x08039C30 fills the per-world saved spots 0x03002CA7 / C87 / C97
/// and view 0x03002CB7, which the world start 0x0802FC00 loads): y 0x0813A000[world], x
/// 0x20 on screen 0, the view's x 8 (World 2: 0).
const START_Y: u32 = 0x0813_A000;
const START_X: u16 = 0x20;
fn start(rom: &Rom, world: u8) -> Result<(u8, u16), ExtractError> {
    Ok((rom.u8(START_Y + world as u32)?, START_X))
}
fn start_scroll(world: u8) -> u8 {
    if world == 1 { 0 } else { 8 }
}
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
const FN_MAP_FORM_PALETTE: u32 = 0x0800_B4F0;
/// World 9 (map world 8) and its pipes' numbers (0x0800C0B4: OAM entries 73-79).
const WARP_ZONE: u8 = 8;
const FN_WARP_ZONE_NUMBERS: u32 = 0x0800_C0B4;
const WARP_ZONE_NUMBERS: std::ops::Range<u32> = 73..80;
const WARP_ZONE_SIGNS: u32 = 0x0813_5D08;
const FN_BG_SCRIPT: u32 = 0x0800_A078;
const SIGN_ROWS: u32 = 4;
const SAVED_SCROLL_X: u32 = 0x0300_3858;
const WORLD1_SCROLL_X: u8 = 8;
const FRAME_COUNTER: u32 = 0x0300_3AE8;
const MAP_SUB_STATE: u32 = 0x0300_385D;
const IDLE: u8 = 0x0E;
const MARIO_SLOTS: std::ops::Range<u32> = 68..70;
/// After a game over (0x080270B8 sets map state +0x17), 0x0800C45C decrements +0x21 and
/// draws the lower OBJ (slot 69) as small Mario turning round, 0x0813A55F[(+0x21 & 6) >> 1].
const GAME_OVER_LOOK: u32 = MAP_STATE + 0x17;
const SPIN_TIMER: u32 = MAP_STATE + 0x21;
const LOWER_SLOT: std::ops::Range<u32> = 69..70;
/// The spade streak (0x03002CD8): bits 2-5 the count. The map init (0x0800AFC4) then puts
/// one tile of LZ77 0x081F9CA8 (offset 0x08135CB4[min(count, 3)] + frame * 0x80) over BG
/// tile 0x68 in every animation frame (0x0201ED00 + frame * 0x1000), and 0x0800B648 gives
/// BG palette 12 the colours 0x0821E980 when the count is odd or 3 and up (palette 4's
/// otherwise): a heart, club or diamond on the spade panel instead of the spade.
const SPADE_STREAK: u32 = 0x0300_2CD8;
/// Animation frames as the map init leaves them in RAM.
const ANIM_RAM: u32 = 0x0201_E000;
/// Completion bits a game over keeps (0x08012458): 0x0813C12E[world * 0x40 + column].
const GAME_OVER_KEEP: u32 = 0x0813_C12E;

fn setup(m: &mut Machine, world: u8) -> Result<(), ExtractError> {
    setup_streak(m, world, 0)
}

/// The map init for `world` started at its start spot (`start`).
fn setup_at(rom: &Rom, m: &mut Machine, world: u8) -> Result<(), ExtractError> {
    setup_streak_at(m, world, 0, start(rom, world)?)
}

/// The map init with the spade streak 0x03002CD8 = `streak` (bits 2-5: the count).
fn setup_streak(m: &mut Machine, world: u8, streak: u8) -> Result<(), ExtractError> {
    setup_streak_at(m, world, streak, (0x40, 0x20))
}

fn setup_streak_at(m: &mut Machine, world: u8, streak: u8, at: (u8, u16)) -> Result<(), ExtractError> {
    m.clear_ram()?;
    m.w8(SPADE_STREAK, streak)?;
    for (ptr, val) in [
        (PLAYER_PTR, 0x0203_A000),
        (PLAYER_GFX_PTR, 0x0203_B000),
        (MAP_STATE_PTR, MAP_STATE),
        (OBJ_BASE_PTR, 0x0203_8000),
    ] {
        m.w32(ptr, val)?;
    }
    m.w8(LANGUAGE, 0)?;
    m.w8(WORLD, world)?;
    // The view's x the map init 0x0800B648 starts from (0x03003858): a world's start view
    // (`start_scroll`); World 9 keeps that of the world the whistle took Mario from, World
    // 1's 8 (mGBA). World 1 itself is extracted at 0 as the port has always drawn it (the
    // ROM's view there is 8 px further right).
    if world != 0 {
        m.w8(SAVED_SCROLL_X, if world == WARP_ZONE { WORLD1_SCROLL_X } else { start_scroll(world) })?;
    }
    m.w8(SAVED_Y, at.0)?;
    m.w8(SAVED_X, at.1 as u8)?;
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
    let count = (0..4)
        .map(|s| rom.u8(starts + s).unwrap_or(0))
        .max()
        .unwrap_or(0) as u32;
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

pub fn extract_world_map(
    rom: &Rom,
    m: &mut Machine,
    world: u8,
    out_dir: &std::path::Path,
) -> Result<(), ExtractError> {
    if world == WARP_ZONE { setup(m, world)? } else { setup_at(rom, m, world)? };
    let nodes = nodes(rom, world)?;
    let screens = nodes
        .iter()
        .map(|n| (n.x >> 8) as usize + 1)
        .max()
        .unwrap_or(1);
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
                atlas[dst..dst + 64]
                    .copy_from_slice(&cell[(y * 64) as usize..(y * 64 + 64) as usize]);
            }
        }
        if f == 0 {
            frame0 = cells;
        }
    }

    // Line the grid up with the ROM's BG1 view (frame 0 of the animation is in VRAM).
    let (bg1, bg1_chars, scroll) = layer(m, 1)?;
    let view = render_view(&bg1, layer_width(m, 1)?, &bg1_chars, &pal, scroll);
    let origin = (-32..=16)
        .flat_map(|oy| (-16..=16).map(move |ox| (ox, oy)))
        .max_by_key(|&o| {
            let ours = render_grid(&grid, cols, &frame0, o);
            ours.as_chunks::<4>()
                .0
                .iter()
                .zip(view.pixels.as_chunks::<4>().0)
                .filter(|(a, b)| a == b)
                .count()
        })
        .unwrap_or((0, 0));
    let ours = render_grid(&grid, cols, &frame0, origin);
    let same = ours
        .as_chunks::<4>()
        .0
        .iter()
        .zip(view.pixels.as_chunks::<4>().0)
        .filter(|(a, b)| a == b)
        .count();
    println!(
        "  world map {} walk: grid {cols}x{ROWS}, origin {origin:?}, {same}/{} px as the ROM's view",
        world + 1,
        240 * 160
    );

    // World 9: the "WORLD 9 WARP ZONE!" sign (BG3, priority 1) and the pipes' world numbers
    // (0x0800C0B4 each frame: OAM 73-79 from 0x0813A577, 8x8 tiles 0xD1-0xDD, palette 1,
    // priority 3), as one picture of the screen.
    if world == WARP_ZONE {
        // The map init writes the sign (0x0800A078(0x08135D08[language])) before the "WORLD
        // n" panel covers BG3 (sub-state 0); once the panel has closed the sign is back.
        let sign = rom.u8(WARP_ZONE_SIGNS + m.read(LANGUAGE, 1)?[0] as u32)?;
        m.call(FN_BG_SCRIPT, &[sign as u32])?;
        // BG3 unscrolled; the panel below the sign (rows 4 on) is cleared once it has closed
        // (mGBA: BG3 then holds the sign alone).
        let (bg3, bg3_chars, _) = layer(m, 3)?;
        let mut overlay = render_view(&bg3, layer_width(m, 3)?, &bg3_chars, &pal, (0, 0));
        overlay.pixels[(SIGN_ROWS * 8 * 240 * 4) as usize..].fill(0);
        m.w8(MAP_STATE + 0x69, 0)?;
        for s in 0..128 {
            m.w16(OAM_BUF + s * 8, 0xA0)?;
        }
        m.call(FN_WARP_ZONE_NUMBERS, &[])?;
        draw_oam(m, &mut overlay, WARP_ZONE_NUMBERS)?;
        overlay
            .write_png(&sma4_assets::map_overlay_path(out_dir, world))
            .map_err(|e| ExtractError(e.to_string()))?;
    }

    // Mario's map sprite per form and animation phase.
    let mut mario = Rgba {
        width: MAP_MARIO_W * MAP_MARIO_FORMS * 2,
        height: MAP_MARIO_H,
        pixels: vec![0; (MAP_MARIO_W * MAP_MARIO_FORMS * 2 * MAP_MARIO_H * 4) as usize],
    };
    m.w8(MAP_SUB_STATE, IDLE)?;
    for (i, form) in (0..MAP_MARIO_FORMS as u8).enumerate() {
        for phase in 0..2u32 {
            for s in 0..128 {
                m.w16(OAM_BUF + s * 8, 0xA0)?;
            }
            // 0x0800B4F0: the form's palette into OBJ palette 7 (the cloud: 0x0821E380).
            // It leaves the P-Wing's alone: the item set Mario's own (0x080062BC:
            // 0x0821E380[0x0813AD00[8] = 0x50]), the raccoon's.
            m.w8(MAP_FORM, if form == 8 { 3 } else { form })?;
            m.call(FN_MAP_FORM_PALETTE, &[])?;
            m.w8(MAP_FORM, form)?;
            m.w8(FRAME_COUNTER, (phase * 8) as u8)?;
            m.call(FN_DRAW_MARIO, &[])?;
            let lower = m.read(OAM_BUF + 69 * 8, 4)?;
            let (ly, lx) = (
                lower[0] as i32,
                (u16::from_le_bytes([lower[2], lower[3]]) & 0x1FF) as i32,
            );
            let mut canvas = Rgba {
                width: 240,
                height: 160,
                pixels: vec![0; 240 * 160 * 4],
            };
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

    // Mario spinning after a game over, one cell per phase of +0x21 (the draw decrements it first).
    let hs = MAP_SPIN_SIZE;
    let mut mario_spin = Rgba {
        width: hs * MAP_SPIN_PHASES,
        height: hs,
        pixels: vec![0; (hs * MAP_SPIN_PHASES * hs * 4) as usize],
    };
    m.w8(MAP_FORM, 0)?;
    m.call(FN_MAP_FORM_PALETTE, &[])?;
    m.w8(GAME_OVER_LOOK, 1)?;
    for phase in 0..MAP_SPIN_PHASES {
        for s in 0..128 {
            m.w16(OAM_BUF + s * 8, 0xA0)?;
        }
        m.w8(SPIN_TIMER, (phase * 2 + 1) as u8)?;
        m.call(FN_DRAW_MARIO, &[])?;
        let lower = m.read(OAM_BUF + 69 * 8, 4)?;
        let (ly, lx) = (
            lower[0] as i32,
            (u16::from_le_bytes([lower[2], lower[3]]) & 0x1FF) as i32,
        );
        let mut canvas = Rgba {
            width: 240,
            height: 160,
            pixels: vec![0; 240 * 160 * 4],
        };
        draw_oam(m, &mut canvas, LOWER_SLOT)?;
        for y in 0..hs as i32 {
            for x in 0..hs as i32 {
                let (sx, sy) = (lx + x, ly + y);
                if !(0..240).contains(&sx) || !(0..160).contains(&sy) {
                    continue;
                }
                let s = ((sy * 240 + sx) * 4) as usize;
                let d = ((y as u32 * mario_spin.width + phase * hs + x as u32) * 4) as usize;
                mario_spin.pixels[d..d + 4].copy_from_slice(&canvas.pixels[s..s + 4]);
            }
        }
    }
    m.w8(GAME_OVER_LOOK, 0)?;

    // The world-start stars: one 16x16 OBJ per tile of 0x0813A676 in star slot 38.
    use sma4_assets::{MAP_STAR_FRAMES, MAP_STAR_SIZE};
    let ss = MAP_STAR_SIZE;
    let mut stars = Rgba {
        width: ss * MAP_STAR_FRAMES,
        height: ss,
        pixels: vec![0; (ss * MAP_STAR_FRAMES * ss * 4) as usize],
    };
    for f in 0..MAP_STAR_FRAMES {
        for s in 0..128 {
            m.w16(OAM_BUF + s * 8, 0xA0)?;
        }
        let tile = rom.u8(STAR_TILES + f)? as u16;
        m.w16(OAM_BUF + STAR_SLOT * 8, 0)?;
        m.w16(OAM_BUF + STAR_SLOT * 8 + 2, STAR_ATTR1)?;
        m.w16(OAM_BUF + STAR_SLOT * 8 + 4, tile | STAR_ATTR2)?;
        let mut canvas = Rgba {
            width: 240,
            height: 160,
            pixels: vec![0; 240 * 160 * 4],
        };
        draw_oam(m, &mut canvas, STAR_SLOT..STAR_SLOT + 1)?;
        for y in 0..ss {
            let s = (y * 240 * 4) as usize;
            let d = ((y * stars.width + f * ss) * 4) as usize;
            stars.pixels[d..d + (ss * 4) as usize]
                .copy_from_slice(&canvas.pixels[s..s + (ss * 4) as usize]);
        }
    }

    let spade_looks = spade_looks(m, world, &entries, char_base)?;
    spade_looks
        .write_png(&sma4_assets::map_spade_path(out_dir, world))
        .map_err(|e| ExtractError(e.to_string()))?;
    // Back to the plain map for what follows.
    setup(m, world)?;

    let game_over_keep = (0..cols as u32)
        .map(|c| rom.u8(GAME_OVER_KEEP + world as u32 * 0x40 + c))
        .collect::<Result<Vec<u8>, _>>()?;

    let anim_ticks: Vec<u8> = (0..MAP_ANIM_FRAMES)
        .map(|f| rom.u8(ANIM_TIMES + world as u32 * 4 + f).map(|t| t + 1))
        .collect::<Result<_, _>>()?;
    WorldMapAsset {
        anim_ticks: [anim_ticks[0], anim_ticks[1], anim_ticks[2], anim_ticks[3]],
        cols,
        rows: ROWS,
        grid,
        start: if world == WARP_ZONE { (0x40, 0x20) } else { start(rom, world)? },
        thresholds,
        nodes,
        origin,
        atlas: Rgba {
            width: aw,
            height: ah,
            pixels: atlas,
        },
        mario,
        game_over_keep,
        mario_spin,
        stars,
    }
    .write(out_dir, world)
    .map_err(|e| ExtractError(e.to_string()))
}

/// The spade panel (metatile `MAP_SPADE_TILE`) by streak count 0-3 (rows) and animation
/// frame (columns), from the map init run with each count.
fn spade_looks(m: &mut Machine, world: u8, entries: &[[u16; 4]], char_base: u32) -> Result<Rgba, ExtractError> {
    use sma4_assets::{MAP_SPADE_LOOKS, MAP_SPADE_TILE};
    let w = 16 * MAP_ANIM_FRAMES;
    let mut px = vec![0u8; (w * 16 * MAP_SPADE_LOOKS * 4) as usize];
    for count in 0..MAP_SPADE_LOOKS {
        setup_streak(m, world, (count as u8) << 2)?;
        let pal = parse_palette(&m.read(BG_PAL_BUFFER, 0x200)?);
        for f in 0..MAP_ANIM_FRAMES {
            let mut chars = m.read(char_base, 0x8000)?;
            chars[..ANIM_BYTES].copy_from_slice(&m.read(ANIM_RAM + f * ANIM_BYTES as u32, ANIM_BYTES)?);
            let cell = metatile(&chars, &pal, entries[MAP_SPADE_TILE as usize]);
            for y in 0..16 {
                let dst = (((count * 16 + y) * w + f * 16) * 4) as usize;
                px[dst..dst + 64].copy_from_slice(&cell[(y * 64) as usize..(y * 64 + 64) as usize]);
            }
        }
    }
    Ok(Rgba { width: w, height: 16 * MAP_SPADE_LOOKS, pixels: px })
}

/// World-start stars (0x08030488): OAM slots 38-45 (0x03002EF8), 16x16 (attr1 0x4000),
/// tile 0x0813A676[toggle], attr2 0x2C00 (palette 2, priority 3).
const STAR_SLOT: u32 = 38;
const STAR_TILES: u32 = 0x0813_A676;
const STAR_ATTR1: u16 = 0x4000;
const STAR_ATTR2: u16 = 0x2C00;

/// Tiles A always enters on (0x0801112C checks 0x0813A616 before the thresholds).
const ENTER_ALWAYS: u32 = 0x0813_A616;
const ENTER_ALWAYS_LEN: u32 = 11;
/// The levels behind the world's enterable spots (course levels only: Toad houses and the
/// spade game, whose ids have bit 15 set or tileset 15, are not levels; the King's castle
/// is the throne room, whose junction chain leads to the airship), first-seen order.
pub fn course_levels(
    rom: &Rom,
    m: &mut Machine,
    world: u8,
) -> Result<Vec<(u16, u8, u16)>, ExtractError> {
    setup(m, world)?;
    let nodes = nodes(rom, world)?;
    let screens = nodes
        .iter()
        .map(|n| (n.x >> 8) as usize + 1)
        .max()
        .unwrap_or(1);
    let grid = read_grid(m, rom, screens)?;
    let t = m.read(THRESHOLDS, 4)?;
    let always: Vec<u8> = (0..ENTER_ALWAYS_LEN)
        .map(|i| rom.u8(ENTER_ALWAYS + i))
        .collect::<Result<_, _>>()?;
    let mut out: Vec<(u16, u8, u16)> = Vec::new();
    for n in nodes {
        let (row, col) = (
            ((n.y as usize).wrapping_sub(0x10) >> 4) & 0xF,
            ((n.x >> 8) as usize) * COLS + (n.x as usize & 0xFF) / 16,
        );
        let Some(&tile) = grid.get(row * COLS * screens + col) else {
            continue;
        };
        let enterable = always.contains(&tile) || tile >= t[(tile >> 6) as usize];
        let course = n.level & 0x8000 == 0
            && n.enemies & 0x8000 == 0
            && n.tileset != 15;
        if enterable && course && !out.iter().any(|l| l.0 == n.level) {
            out.push((n.level, n.tileset, n.enemies));
        }
    }
    Ok(out)
}

/// The battle arenas: the level-table entries of the path junctions (tiles that cannot be
/// entered), where a map object meeting Mario starts a battle (0x08031DDC -> map sub-state
/// 0x16 with 0x03003AF7 = the object's kind; 0x08009A3C then loads the entry for Mario's
/// cell as for any spot). World 1: levels 0x4E and 0x4F, tileset 3, enemy list 0x61.
/// Tiles (second plane << 8 | tile) on which a map object can meet Mario (0x08031D5C).
const CONTACT_TILES: u32 = 0x0814_26EC;
const CONTACT_TILES_LEN: u32 = 13;

pub fn battle_levels(rom: &Rom, m: &mut Machine, world: u8) -> Result<Vec<(u16, u8, u16)>, ExtractError> {
    setup(m, world)?;
    let nodes = nodes(rom, world)?;
    let screens = nodes.iter().map(|n| (n.x >> 8) as usize + 1).max().unwrap_or(1);
    let grid = read_grid(m, rom, screens)?;
    let t = m.read(THRESHOLDS, 4)?;
    let always: Vec<u8> = (0..ENTER_ALWAYS_LEN).map(|i| rom.u8(ENTER_ALWAYS + i)).collect::<Result<_, _>>()?;
    let mut out: Vec<(u16, u8, u16)> = Vec::new();
    for n in nodes {
        let (row, col) =
            (((n.y as usize).wrapping_sub(0x10) >> 4) & 0xF, ((n.x >> 8) as usize) * COLS + (n.x as usize & 0xFF) / 16);
        let Some(&tile) = grid.get(row * COLS * screens + col) else {
            continue;
        };
        let enterable = always.contains(&tile) || tile >= t[(tile >> 6) as usize];
        let meets = (0..CONTACT_TILES_LEN).any(|i| rom.u16(CONTACT_TILES + i * 2).is_ok_and(|c| c == tile as u16));
        if !enterable && meets && n.level & 0x8000 == 0 && n.enemies & 0x8000 == 0 && !out.iter().any(|l| l.0 == n.level) {
            out.push((n.level, n.tileset, n.enemies));
        }
    }
    Ok(out)
}

/// Map objects of a new game (0x08039AC8): six slots per world, from the per-world tables
/// below; 0x0802FC00 copies the world's items (nine bytes) to 0x03003790.
const OBJ_Y: u32 = 0x0813_9F60;
const OBJ_SCREEN: u32 = 0x0813_9F80;
const OBJ_X: u32 = 0x0813_9FA0;
const OBJ_KIND: u32 = 0x0813_9FC0;
const OBJ_ITEM: u32 = 0x0813_9FE0;
const OBJ_SLOTS: u32 = 6;
/// Sprite of map object kind k, frame f (0x08031DDC): i = (k - 1) * 3 + f; tile =
/// 0x0813A678[i] & 0xF | (0x0813A678[i] & 0xF0) << 1 | (0x0813A6A8[i] & 1) << 4, palette
/// (0x0813A6A8[i] & 0xE) >> 1; 16x16 (attr1 0x4000), priority 3.
const OBJ_TILES: u32 = 0x0813_A678;
const OBJ_ATTRS: u32 = 0x0813_A6A8;
const OBJ_SLOT: u32 = 40;

pub fn extract_map_objects(rom: &Rom, m: &mut Machine, world: u8, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    use sma4_assets::{MAP_OBJECT_FRAMES, MAP_OBJECT_KINDS, MAP_OBJECT_SIZE, MapObjectSpawn, MapObjectsAsset, map_object_cell};
    setup(m, world)?;
    let w = world as u32 * 4;
    let mut objects = Vec::new();
    for slot in 0..OBJ_SLOTS {
        let at = |t: u32| -> Result<u8, ExtractError> { Ok(rom.u8(rom.ptr(t + w)? + slot)?) };
        let kind = at(OBJ_KIND)?;
        if kind != 0 {
            let x = (at(OBJ_SCREEN)? as u16) << 8 | at(OBJ_X)? as u16;
            objects.push(MapObjectSpawn { slot: slot as u8, kind, y: at(OBJ_Y)?, x, item: at(OBJ_ITEM)? });
        }
    }
    let s = MAP_OBJECT_SIZE;
    let cells = MAP_OBJECT_KINDS * MAP_OBJECT_FRAMES;
    let mut sprites = Rgba { width: s * cells, height: s, pixels: vec![0; (s * cells * s * 4) as usize] };
    for kind in 1..MAP_OBJECT_KINDS as u8 {
        for f in 0..MAP_OBJECT_FRAMES {
            let i = (kind as u32 - 1) * 3 + f;
            let (t, a) = (rom.u8(OBJ_TILES + i)? as u16, rom.u8(OBJ_ATTRS + i)? as u16);
            let tile = t & 0xF | (t & 0xF0) << 1 | (a & 1) << 4;
            for slot in 0..128 {
                m.w16(OAM_BUF + slot * 8, 0xA0)?;
            }
            m.w16(OAM_BUF + OBJ_SLOT * 8, 0)?;
            m.w16(OAM_BUF + OBJ_SLOT * 8 + 2, 0x4000)?;
            m.w16(OAM_BUF + OBJ_SLOT * 8 + 4, tile | 0xC00 | (a & 0xE) << 11)?;
            let mut canvas = Rgba { width: 240, height: 160, pixels: vec![0; 240 * 160 * 4] };
            draw_oam(m, &mut canvas, OBJ_SLOT..OBJ_SLOT + 1)?;
            let cell = map_object_cell(kind, f);
            for y in 0..s {
                let src = (y * 240 * 4) as usize;
                let dst = ((y * sprites.width + cell * s) * 4) as usize;
                sprites.pixels[dst..dst + (s * 4) as usize].copy_from_slice(&canvas.pixels[src..src + (s * 4) as usize]);
            }
        }
    }
    println!("  world map {} objects: {:?}", world + 1, objects.iter().map(|o| (o.kind, o.y, o.x)).collect::<Vec<_>>());
    MapObjectsAsset { objects, sprites }.write(out_dir, world).map_err(|e| ExtractError(e.to_string()))?;
    if world == 0 {
        extract_map_puff(rom, m, out_dir)?;
    }
    Ok(())
}

/// The map's puffs: four 16x16 OBJs around a cell, the same tile in each, flipped none / h
/// / v / both (attr1 0x4000 / 0x5000 / 0x6000 / 0x7000), priority 0. A map object beaten
/// in battle (map sub-state 8, 0x0800F61C): palette 3, tile 0x0813A799[step] as t & 0xF |
/// (t & 0xF0) << 1 | 0x10 (0x16, 0x16, 0x18, 0x1A, 0x1C); an item used on the map
/// (0x08005E10): palette 1, tiles 0x0813AD24 (0x16 .. 0x1E). One 32x32 cell per tile
/// 0x16, 0x18, 0x1A, 0x1C, 0x1E; a row per palette (3, then 1).
const PUFF_TILE0: u16 = 0x16;
const PUFF_PALETTES: [u16; 2] = [3, 1];

fn extract_map_puff(_rom: &Rom, m: &mut Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    use sma4_assets::{MAP_PUFF_FRAMES, MAP_PUFF_SIZE};
    let s = MAP_PUFF_SIZE;
    let rows = PUFF_PALETTES.len() as u32;
    let mut sheet =
        Rgba { width: s * MAP_PUFF_FRAMES, height: s * rows, pixels: vec![0; (s * MAP_PUFF_FRAMES * s * rows * 4) as usize] };
    for (row, pal) in PUFF_PALETTES.into_iter().enumerate() {
        for f in 0..MAP_PUFF_FRAMES {
            let tile = PUFF_TILE0 + 2 * f as u16;
            for slot in 0..128 {
                m.w16(OAM_BUF + slot * 8, 0xA0)?;
            }
            for (i, (y, attr1)) in [(0, 0x4000), (0, 0x5010), (0x10, 0x6000), (0x10, 0x7010)].into_iter().enumerate() {
                let at = OAM_BUF + (OBJ_SLOT + i as u32) * 8;
                m.w16(at, y)?;
                m.w16(at + 2, attr1)?;
                m.w16(at + 4, tile | pal << 12)?;
            }
            let mut canvas = Rgba { width: 240, height: 160, pixels: vec![0; 240 * 160 * 4] };
            draw_oam(m, &mut canvas, OBJ_SLOT..OBJ_SLOT + 4)?;
            for y in 0..s {
                let src = (y * 240 * 4) as usize;
                let dst = (((row as u32 * s + y) * sheet.width + f * s) * 4) as usize;
                sheet.pixels[dst..dst + (s * 4) as usize].copy_from_slice(&canvas.pixels[src..src + (s * 4) as usize]);
            }
        }
    }
    sheet.write_png(&sma4_assets::map_puff_path(out_dir)).map_err(|e| ExtractError(e.to_string()))
}

/// The map's item panel (see `sma4_core::inventory`): BG0's panel map (LZ77 0x0821F674,
/// 0x0800731C), the item icons in it (0x0800602C: BG entries 0x0813AC28[item * 4..],
/// top-left, top-right, bottom-left, bottom-right), the cursor (0x08006A80: the item as a
/// 16x16 OBJ, tile ((item & 7) << 1 | ((item & 8) << 2 | 0xC0) << 1 | 0x10), palette
/// 0x081324DC[item]) and the Warp Whistle's whirlwind (0x080117B4: a 16x16 OBJ, tile
/// 0x0813A9FC[1] / 0x0813A9FE[1] by frame (low nibble | high nibble << 1), palette
/// (0x0813A9FA[1] & 0xE) >> 1), all drawn from World 1's map VRAM.
const PANEL_MAP: u32 = 0x0821_F674;
const ITEM_ICONS: u32 = 0x0813_AC28;
const CURSOR_PALETTES: u32 = 0x0813_24DC;
const WHIRL_ATTR: u32 = 0x0813_A9FA;
const WHIRL_TILES: [u32; 2] = [0x0813_A9FC, 0x0813_A9FE];
const OBJ_VRAM: u32 = 0x0601_0000;
const OBJ_PAL_BUFFER: u32 = 0x0200_0200;

pub fn extract_item_panel(rom: &Rom, m: &mut Machine, out_dir: &std::path::Path) -> Result<(), ExtractError> {
    use sma4_assets::{ITEM_KINDS, item_panel_path};
    setup(m, 0)?;
    let pal = parse_palette(&m.read(BG_PAL_BUFFER, 0x200)?);
    let obj_pal = parse_palette(&m.read(OBJ_PAL_BUFFER, 0x200)?);
    let cnt = u16::from_le_bytes(m.read(BG0_CNT, 2)?.try_into().unwrap_or([0, 0]));
    let chars = m.read(VRAM + ((cnt as u32 >> 2) & 3) * 0x4000, 0x8000)?;
    let obj = m.read(OBJ_VRAM, 0x8000)?;
    let blit = |px: &mut Vec<u8>, w: usize, x0: usize, y0: usize, t: &[u8; 256]| {
        for y in 0..8 {
            for x in 0..8 {
                let s = (y * 8 + x) * 4;
                if t[s + 3] != 0 {
                    px[((y0 + y) * w + x0 + x) * 4..][..4].copy_from_slice(&t[s..s + 4]);
                }
            }
        }
    };
    // The panel: its map's rows, 32 entries each.
    let map: Vec<u16> = rom.decompress(PANEL_MAP)?.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b)).collect();
    let rows = (map.len() / 32).min(32);
    let mut panel = vec![0u8; 256 * rows * 8 * 4];
    for (i, &e) in map.iter().take(rows * 32).enumerate() {
        blit(&mut panel, 256, (i % 32) * 8, (i / 32) * 8, &bg_tile(&chars, &pal, e));
    }
    // Icons and cursors, a 16x16 cell per item; the whirlwind's two frames after them.
    let n = ITEM_KINDS as usize;
    let w = 16 * (n + 2);
    let mut sheet = vec![0u8; w * 32 * 4];
    for item in 1..n {
        let a = ITEM_ICONS + item as u32 * 8;
        let e = [rom.u16(a)?, rom.u16(a + 2)?, rom.u16(a + 4)?, rom.u16(a + 6)?];
        for (k, &entry) in e.iter().enumerate() {
            blit(&mut sheet, w, item * 16 + (k % 2) * 8, (k / 2) * 8, &bg_tile(&chars, &pal, entry));
        }
        if item < 0x0E {
            let i = item as u16;
            let tile = (i & 7) << 1 | ((i & 8) << 2 | 0xC0) << 1 | 0x10;
            let p = (rom.u8(CURSOR_PALETTES + item as u32)? as u16) << 12;
            for (k, off) in [0u16, 1, 32, 33].into_iter().enumerate() {
                blit(&mut sheet, w, item * 16 + (k % 2) * 8, 16 + (k / 2) * 8, &bg_tile(&obj, &obj_pal, (tile + off) | p));
            }
        }
    }
    let attr = rom.u8(WHIRL_ATTR + 1)? as u16;
    for (f, table) in WHIRL_TILES.iter().enumerate() {
        let b = rom.u8(table + 1)? as u16;
        let tile = (b & 0xF) | (b & 0xF0) << 1;
        let p = ((attr & 0xE) >> 1) << 12;
        for (k, off) in [0u16, 1, 32, 33].into_iter().enumerate() {
            blit(&mut sheet, w, (n + f) * 16 + (k % 2) * 8, (k / 2) * 8, &bg_tile(&obj, &obj_pal, (tile + off) | p));
        }
    }
    let write = |name: &str, width: usize, height: usize, pixels: Vec<u8>| {
        Rgba { width: width as u32, height: height as u32, pixels }
            .write_png(&item_panel_path(out_dir, name))
            .map_err(|e| ExtractError(e.to_string()))
    };
    write("panel", 256, rows * 8, panel)?;
    write("items", w, 32, sheet)?;
    println!("  item panel: {rows} rows, {} items", n - 1);
    Ok(())
}

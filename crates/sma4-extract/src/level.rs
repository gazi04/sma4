//! Builds a ready-to-draw level by running the ROM's own loaders (see docs/re-notes.md,
//! "Level storage"), then rendering every distinct metatile into an atlas.
//! Addresses are for the European ROM (AX4P rev 0).

use sma4_assets::{
    ANIM_FRAMES, BACKGROUND_PX, Background, BlockRules, EnemySpawn, FAR_BACKGROUND_H, FAR_BACKGROUND_W,
    FarBackground, Junction, LevelAsset, METATILE,
    METATILES, PipeExit, Rgba, SLOPE_SHAPES, Slopes, Solidity,
};
use sma4_rom::{
    ROM_BASE, Rom,
    gfx::parse_palette,
    level::{Metatile, render_metatile},
};

use crate::emu::{EmuError, Machine};

const LEVEL_TABLE: u32 = 0x0813_6200;
const HEADER_LEN: u32 = 11;

// Loader RAM variables.
const V_DATA_PTR: u32 = 0x0300_3B04;
const V_PARAM: u32 = 0x0300_3846;
const V_BANK: u32 = 0x0300_3847;
const V_ID: u32 = 0x0300_3848;
const V_TILESET: u32 = 0x0300_3849;
const V_Y: u32 = 0x0300_3AD0;
const V_X: u32 = 0x0300_3AD1;
const V_VERTICAL: u32 = 0x0300_3CB3;
const V_LEVEL_STATE: u32 = 0x0300_7818;
const V_GFX_SET: u32 = 0x0300_37DF;
const V_BG_SCENE: u32 = 0x0300_3BBF;
const V_LEVEL_ID: u32 = 0x0300_3B0A;
const V_PAL_VARIANT: u32 = 0x0300_3869;
const V_SKY_COLOUR: u32 = 0x0300_386A;
const V_EXTRA_PALETTE: u32 = 0x0300_3B99;
const V_SCENE_MODE: u32 = 0x0300_3B9A;
const V_SCENE_PALETTE: u32 = 0x0300_3BA4;
/// BGxCNT shadows: each layer gets its own char/screen base so the loaders don't
/// overwrite each other. Level layer (BG1) uses char block 0.
const BG_CNT_SHADOWS: [(u32, u16); 4] = [
    (0x0300_2DA4, 0x1F08),
    (0x0300_2DA6, 0x1C00),
    (0x0300_2DA8, 0x1D08),
    (0x0300_2DAA, 0x1E08),
];

// Loader routines.
const FN_POSITION: u32 = 0x0801_4D24;
const FN_BANK0: u32 = 0x0801_4A20;
const FN_BANK1: u32 = 0x0801_49F8;
const FN_FILL_ROWS: u32 = 0x0801_4A48;
const FN_FILL_PAGES: u32 = 0x0801_4B94;
const FN_LOAD_BG_GFX: u32 = 0x0802_E860;
const FN_LOAD_PALETTES: u32 = 0x0800_85AC;

/// Background tile animation (VBlank 0x080016DC, sources set up by 0x08016434): every 8
/// frames, 4 tiles of the question block go to char offset 0x1300 (tile 0x98) and 4 coin
/// tiles to 0x1B80 (tile 0xDC). Frame f's tiles are at offsets 0x08135F26[f] (block) and
/// 0x08135F2E[f] (coin), u16, in LZ77 0x0818B604.
const ANIM_SOURCE: u32 = 0x0818_B604;
const ANIM_BLOCK_OFFSETS: u32 = 0x0813_5F26;
const ANIM_COIN_OFFSETS: u32 = 0x0813_5F2E;
const ANIM_BLOCK_DEST: usize = 0x1300;
const ANIM_COIN_DEST: usize = 0x1B80;
const ANIM_BYTES: usize = 0x80;

/// Graphics sets without the animation (checks in 0x080016DC).
fn animates(gfx_set: u8) -> bool {
    !matches!(gfx_set, 7 | 0x10..=0x12 | 0x16 | 0x2C)
}

/// Time limit hundreds by header byte 8 >> 6 (0x08014F20): 3, 4, 2, none.
const TIME_LIMITS: u32 = 0x0813_6016;
/// Pipe flags per tileset class (0x0809B598) and the class of each tileset (0x08094780).
const PIPE_FLAGS: u32 = 0x0817_2E44;
const TILESET_CLASS: u32 = 0x0817_2BCC;
/// Vertical scroll settings per level id (0x08014F20, the classic game's table).
const CAMERA_PARAMS: u32 = 0x0813_6F30;
/// Water ids per tileset class and id quadrant (0x080A1E60).
const WATER_IDS: u32 = 0x0813_DD04;
/// Camera start y per header byte 4 >> 5 (0x08014F20 -> 0x03003754).
const START_CAMERA_Y: u32 = 0x0813_600E;
/// Player start y per header byte 4 >> 5 (0x08014F20).
const START_Y: u32 = 0x0813_789A;

const SCRATCH_STATE: u32 = 0x0203_A000;
const PAGE_BUFFERS: u32 = 0x0202_E000; // 16 pages x 0x400 bytes of metatile ids
const HIGH_LAYER: u32 = PAGE_BUFFERS + 0x4000;
const PAL_BUFFER: u32 = 0x0200_0000;
const PAGES: usize = 16;
const ROWS: usize = 27;
const SKY: Metatile = Metatile {
    id: 0x80,
    high: false,
};

#[derive(Debug)]
pub struct ExtractError(pub String);

impl From<EmuError> for ExtractError {
    fn from(e: EmuError) -> Self {
        Self(e.0)
    }
}

impl From<sma4_rom::RomError> for ExtractError {
    fn from(e: sma4_rom::RomError) -> Self {
        Self(e.to_string())
    }
}

type Result<T> = std::result::Result<T, ExtractError>;

/// Pipe exit y per pointer bits 6-4 (0x08005290): low nibble = y page, high nibble = y
/// low byte.
const EXIT_Y: u32 = 0x0813_1EC1;
const EXIT_CAMERA_Y: u32 = 0x0813_1ECA;

/// Pointer records (kind 0x80, 0x08014CE0): `(page, value_a, value_b)` = row byte, the
/// 0x0300381E and the 0x0300382E entries.
type Pointers = Vec<(u8, u8, u8)>;

/// Replays the object loop of the level loader (0x08014F20) record by record.
fn load_objects(
    m: &mut Machine,
    rom: &Rom,
    data: u32,
    tileset: u8,
    pointers: &mut Pointers,
) -> Result<usize> {
    m.w8(V_TILESET, tileset)?;
    m.w8(V_VERTICAL, rom.u8(data + 6)? & 0x10)?;
    let mut i = data + HEADER_LEN;
    let first = rom.u8(i)?;
    if first != 0xFF && first & 0xC0 == 0xC0 {
        i += 3;
    }
    let mut records = 0;
    loop {
        let b = rom.u8(i)?;
        if b == 0xFF {
            return Ok(records);
        }
        let rec = rom.slice(i, 4)?;
        let bank = b & 0xC0;
        if bank == 0x80 {
            // Pointer: where a pipe entered on page `rec[1]` leads; later records win.
            pointers.retain(|p| p.0 != rec[1] & 0x0F);
            pointers.push((rec[1] & 0x0F, rec[2], rec[3]));
            i += 4;
            continue;
        }
        m.w8(V_BANK, bank)?;
        m.w8(V_PARAM, b & 0x3F)?;
        m.w8(V_Y, rec[1])?;
        m.w8(V_X, rec[2])?;
        m.w8(V_ID, rec[3])?;
        m.w8(V_TILESET, tileset)?;
        m.w32(V_DATA_PTR, i + 4)?;
        m.call(FN_POSITION, &[])?;
        if bank < 0x40 {
            m.call(FN_BANK0, &[])?;
        } else {
            m.w8(V_BANK, bank | 1)?;
            m.call(FN_BANK1, &[])?;
        }
        // Generators may consume extra bytes; the loader resumes where they leave the pointer.
        let next = m.r32(V_DATA_PTR)?;
        if !(i + 4..=i + 12).contains(&next) {
            return Err(ExtractError(format!(
                "object at {i:#x} moved the data pointer to {next:#x}"
            )));
        }
        i = next;
        records += 1;
    }
}

pub fn extract(
    rom: &Rom,
    m: &mut Machine,
    level_id: u16,
    tileset: u8,
    enemy_list: u16,
) -> Result<LevelAsset> {
    extract_at(rom, m, level_id, tileset, rom.ptr(ENEMY_LISTS + enemy_list as u32 * 4)?)
}

/// `extract` with the enemy list given by address.
pub fn extract_at(
    rom: &Rom,
    m: &mut Machine,
    level_id: u16,
    tileset: u8,
    enemy_list: u32,
) -> Result<LevelAsset> {
    let data = rom.ptr(LEVEL_TABLE + level_id as u32 * 4)?;
    let hdr = rom.slice(data, HEADER_LEN as usize)?.to_vec();

    m.clear_ram()?;
    m.w32(V_LEVEL_STATE, SCRATCH_STATE)?;
    // Same pre-fill as the tileset setup (0x080174EC): every page starts as sky.
    for i in 0..0xF0 {
        m.call(FN_FILL_ROWS, &[SKY.id as u32, i])?;
    }
    for i in 0..0x340 {
        m.call(FN_FILL_PAGES, &[SKY.id as u32, i])?;
    }
    let mut pointers = Vec::new();
    let records = load_objects(m, rom, data, tileset, &mut pointers)?;

    let gfx_set = ((hdr[8] & 0x30) << 1) | (hdr[7] & 0x1F);
    m.w8(V_TILESET, tileset)?;
    m.w8(V_GFX_SET, gfx_set)?;
    m.w8(V_BG_SCENE, hdr[10])?;
    // Header fields the palette loader reads (0x08014F20): byte 5 bits 1-0 / 4-2, byte 9
    // high / low nibble, and the scene's palette (0x08025460: 0x08139A2C[scene][0], used by
    // 0x08007D2C for BG palettes 1-2).
    m.w8(V_PAL_VARIANT, hdr[5] & 3)?;
    m.w8(V_SKY_COLOUR, (hdr[5] & 0x1C) >> 2)?;
    m.w8(V_EXTRA_PALETTE, hdr[9] >> 4)?;
    m.w8(V_SCENE_MODE, hdr[9] & 0xF)?;
    // Scene 0 (the King's throne room) has no data: no background, palette 0.
    let scene_data = rom.u32(SCENE_DATA + hdr[10] as u32 * 4)?;
    m.w8(V_SCENE_PALETTE, if scene_data == 0 { 0 } else { rom.u8(scene_data)? })?;
    for (addr, v) in BG_CNT_SHADOWS {
        m.w16(addr, v)?;
    }
    m.w16(V_LEVEL_ID, level_id)?;
    m.call(FN_LOAD_BG_GFX, &[])?;
    m.call(FN_LOAD_PALETTES, &[])?;

    let background = background(rom, m, hdr[10], hdr[9] & 0xF)?;
    let pages = m.read(PAGE_BUFFERS, 0x4000)?;
    let high = m.read(HIGH_LAYER, 0x4000)?;
    let palette = parse_palette(&m.read(PAL_BUFFER, 0x200)?);
    let mut chars = m.read(0x0600_0000, 0x8000)?;
    let at = |x: usize, y: usize| {
        let o = (x >> 4) * 0x400 + y * 16 + (x & 15);
        Metatile {
            id: pages[o],
            high: high[o] != 0,
        }
    };

    // Width: the level's last screen page, header byte 4 bits 3-0 (0x08014F20 ->
    // 0x03003AE6; the camera stops there, 0x0809869C). Some levels fill all 16 pages with
    // a background metatile, so the content alone does not tell where they end.
    let width = ((hdr[4] & 0x0F) as usize + 1) * 16;
    debug_assert!(width <= PAGES * 16);

    let solidity = rom.solidity(tileset)?;
    let metatiles: Vec<u16> = (0..ROWS)
        .flat_map(|y| (0..width).map(move |x| (y, x)))
        .map(|(y, x)| encode(at(x, y)))
        .collect();

    // Every metatile of the tileset, so the game can swap cells freely (e.g. a hit
    // question block turning into an empty block). Layer = id | 0x100 * high.
    // Rendered once per animation frame, with that frame's tiles patched in.
    let anim = rom.decompress(ANIM_SOURCE)?;
    let mut atlas =
        Vec::with_capacity((METATILE * METATILE * 4 * METATILES * ANIM_FRAMES) as usize);
    for f in 0..ANIM_FRAMES {
        if animates(gfx_set) {
            for (offsets, dest) in [
                (ANIM_BLOCK_OFFSETS, ANIM_BLOCK_DEST),
                (ANIM_COIN_OFFSETS, ANIM_COIN_DEST),
            ] {
                let src = rom.u16(offsets + f * 2)? as usize;
                chars[dest..dest + ANIM_BYTES].copy_from_slice(&anim[src..src + ANIM_BYTES]);
            }
        }
        for m in 0..METATILES as u16 {
            let mt = Metatile {
                id: m as u8,
                high: m >= 0x100,
            };
            atlas.extend(render_metatile(
                &chars,
                &palette,
                rom.metatile_entries(tileset, mt)?,
            ));
        }
    }

    // TODO(re): the backdrop is set by the background-scene loader (not ported). Use
    // colour 0 of the palette the sky metatile is drawn with.
    let sky_pal = rom.metatile_entries(tileset, SKY)?[0].palette as usize;
    let [r, g, b, _] = palette[sky_pal * 16].to_rgba(255);

    let t = rom.block_tables(tileset)?;
    let rules = BlockRules {
        bump_behaviour: t.bump_behaviour,
        land_behaviour: t.land_behaviour,
        result_hi: t.result_hi,
        item_types: t.item_types,
        final_kind: t.final_kind,
        final_metatile: t.final_metatile,
        bounce: t.bounce,
    };

    let enemies = rom
        .enemy_list_at(enemy_list)?
        .into_iter()
        .map(|e| EnemySpawn {
            kind: e.kind,
            col: e.col as u16,
            row: e.row,
            extra: e.extra.map(|b| b.unwrap_or(0)),
        })
        .collect::<Vec<_>>();

    let distinct = metatiles
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len();
    println!(
        "  level {level_id:#05x}: {records} objects, {width}x{ROWS} metatiles, {distinct} distinct, gfx set {gfx_set}, {} enemies, pipe pointers {pointers:02x?}",
        enemies.len()
    );
    Ok(LevelAsset {
        level_id,
        tileset,
        width,
        height: ROWS,
        backdrop: [r, g, b],
        solidity: Solidity {
            top: solidity.top,
            solid: solidity.solid,
            high: high_ids(rom, tileset)?,
        },
        metatiles,
        rules,
        enemies,
        time_limit: rom.u8(TIME_LIMITS + (hdr[8] >> 6) as u32)? as u16 * 100,
        start_y: rom.u16(START_Y + (hdr[4] >> 5) as u32 * 2)?,
        start_camera_y: rom.u8(START_CAMERA_Y + (hdr[4] >> 5) as u32)? as u16,
        start_kind: hdr[7] >> 5,
        pipe_flags: rom.u8(PIPE_FLAGS + rom.u8(TILESET_CLASS + tileset as u32)? as u32)?,
        class: rom.u8(TILESET_CLASS + tileset as u32)?,
        camera: {
            let t = rom.ptr(CAMERA_PARAMS + level_id as u32 * 4)?;
            let w = |i: u32| rom.u16(t + i);
            sma4_assets::CameraParams {
                bottom: w(0)?,
                fixed: w(2)?,
                dead_zone: [w(4)?, w(6)?],
                dead_zone_flying: [w(0x16)?, w(0x18)?],
                top: w(0x1E)?,
                mode: (hdr[6] & 0x60) >> 5,
            }
        },
        water: {
            let class = rom.u8(TILESET_CLASS + tileset as u32)? as u32;
            let mut w = [0; 4];
            for (q, b) in w.iter_mut().enumerate() {
                *b = rom.u8(WATER_IDS + class * 4 + q as u32)?;
            }
            w
        },
        junction: junction(rom, &hdr, &pointers)?,
        camera_x: if data == BONUS_ROOM_1_1 {
            Some(BONUS_ROOM_CAMERA)
        } else {
            pinned_camera(data).then_some(PINNED_CAMERA)
        },
        slopes: slopes(rom, tileset, solidity.top)?,
        background,
        atlas: Rgba {
            width: METATILE,
            height: METATILE * METATILES * ANIM_FRAMES,
            pixels: atlas,
        },
    })
}

/// Enemy-list pointers by id (`Rom::enemy_list`).
const ENEMY_LISTS: u32 = 0x0813_6B14;

/// 0x080A4D4C special-cases the camera of World 1-1's coin room (object stream
/// 0x08155460, level 0x23): x from 0x70 (+0xA0 of the player struct) to 0xA0 (+0x9E).
const BONUS_ROOM_1_1: u32 = 0x0815_5460;
const BONUS_ROOM_CAMERA: (u16, u16) = (0x70, 0xA0);

/// The level load 0x08016434 (0x030037DC clear) pins the camera at x 8 (0x03003750 = 8,
/// player +0x9E = +0xA0 = 8) for level data (0x03003B0C) in 0x081623FA..0x081624C6,
/// 0x08162611..0x08162691 or at 0x081625D9 / 0x08161224 / 0x081626D1: the airships'
/// boss cabins (Larry's 0x213, ...) and levels 0x203 and 0x21A..0x21E. (Data in
/// 0x0815D87B..0x0815D96B, levels 0x15E..0x160, pins it 8 px right of where it starts:
/// not handled.) Checked in mGBA: +0x9E = +0xA0 = 8 in Larry's cabin.
fn pinned_camera(data: u32) -> bool {
    (0x0816_23FA..=0x0816_24C6).contains(&data)
        || (0x0816_2611..=0x0816_2691).contains(&data)
        || [0x0816_25D9, 0x0816_1224, 0x0816_26D1].contains(&data)
}
const PINNED_CAMERA: (u16, u16) = (8, 8);

/// The alternate area named by header bytes 0-3 / 6 (0x08014F20 -> 0x030037CE, 0x030037D0,
/// 0x030037DE) and the pipe exits into it, decoded like 0x08005290: value_b low nibble =
/// x page, high nibble | 8 = x low byte; value_a low nibble = exit direction (sideways
/// exits clear x bits 0-3), bits 6-4 index the y table.
fn junction(rom: &Rom, hdr: &[u8], pointers: &Pointers) -> Result<Option<Junction>> {
    let level = u16::from_be_bytes([hdr[0], hdr[1]]);
    if level == 0 || pointers.is_empty() {
        return Ok(None);
    }
    let mut exits = Vec::new();
    for &(page, a, b) in pointers {
        let dir = a & 0x0F;
        let mut x = ((b & 0x0F) as u16) << 8 | (b & 0xF0) as u16 | 8;
        if dir > 2 {
            x &= !0x0F;
        }
        let k = ((a & 0x70) >> 4) as u32;
        let v = rom.u8(EXIT_Y + k)?;
        let y = ((v & 0x0F) as u16) << 8 | (v & 0xF0) as u16;
        // Camera: 0x08131ECA[k], 0x20 lower when y > 0x6F; below the first 256 px (and not
        // a vertical area) 0xF0, or 0x110 when the low byte of y is above 0x6F.
        let camera_y = if y >= 0x100 {
            if y & 0xFF > 0x6F { 0x110 } else { 0xF0 }
        } else {
            rom.u16(EXIT_CAMERA_Y + k * 2)? + if y > 0x6F { 0x20 } else { 0 }
        };
        exits.push(PipeExit {
            page,
            x,
            y,
            dir,
            camera_y,
        });
    }
    exits.sort_by_key(|e| e.page);
    Ok(Some(Junction {
        level,
        enemies: u16::from_be_bytes([hdr[2], hdr[3]]),
        tileset: hdr[6] & 0x0F,
        exits,
    }))
}

fn encode(m: Metatile) -> u16 {
    m.id as u16 | if m.high { 0x100 } else { 0 }
}

/// ROM address of a level's object stream (for messages).
pub fn level_addr(rom: &Rom, level_id: u16) -> u32 {
    rom.ptr(LEVEL_TABLE + level_id as u32 * 4)
        .unwrap_or(ROM_BASE)
}

/// Second-plane metatiles 0x00-0x25 collide like 0x0813E174[tileset][id] (0x080A06E8).
const HIGH_ID_TABLES: u32 = 0x0813_E174;

fn high_ids(rom: &Rom, tileset: u8) -> Result<[u8; sma4_assets::HIGH_IDS]> {
    let table = rom.ptr(HIGH_ID_TABLES + tileset as u32 * 4)?;
    let mut out = [0u8; sma4_assets::HIGH_IDS];
    for (i, b) in out.iter_mut().enumerate() {
        *b = rom.u8(table + i as u32)?;
    }
    Ok(out)
}

/// 0x080A1CEC: slopes are on in the hills (3) and underground (14) tilesets.
// TODO(re): also tileset 5 when 0x030037DA == 2 (not traced yet).
const SLOPE_TILESETS: [u8; 2] = [3, 14];
/// Slope shape per id above the "top" threshold, one table per quadrant (0x080A3760).
const SLOPE_SHAPE_TABLES: u32 = 0x0816_9E68;
const SLOPE_HEIGHTS: u32 = 0x0816_A029;
const SLOPE_NO_FLOOR: u32 = 0x0817_2DE3;
const SLOPE_PUSH: u32 = 0x0816_A179;

fn slopes(rom: &Rom, tileset: u8, top: [u8; 4]) -> Result<Option<Slopes>> {
    if !SLOPE_TILESETS.contains(&tileset) {
        return Ok(None);
    }
    let mut s = Slopes::default();
    for id in 0..=255u8 {
        let q = (id >> 6) as usize;
        if id >= top[q] {
            let table = rom.ptr(SLOPE_SHAPE_TABLES + q as u32 * 4)?;
            s.shape[id as usize] = rom.u8(table + (id - top[q]) as u32)?;
        }
    }
    for k in 0..SLOPE_SHAPES {
        for (x, h) in s.height[k].iter_mut().enumerate() {
            *h = rom.u8(SLOPE_HEIGHTS + (k * 16 + x) as u32)?;
        }
        s.no_floor[k] = rom.u8(SLOPE_NO_FLOOR + k as u32)? != 0;
        s.push[k] = rom.u8(SLOPE_PUSH + k as u32)? as i8;
    }
    Ok(Some(s))
}

/// Background scenes (0x08025460): data 0x08139A2C[scene] (byte 1 -> 0x03003BBD), loader
/// 0x08139C90[scene], which unpacks the 64x64 BG2 map into 0x0201C000 (scene 5: LZ77
/// 0x08221738, scene 10: 0x0822246C). Its tiles come with the graphics set, in BG char
/// block 0.
const SCENE_DATA: u32 = 0x0813_9A2C;
const AIRSHIP_SKY_MODE: u8 = 3;
const SCENE_LOADERS: u32 = 0x0813_9C90;
const SCENE_MAP: u32 = 0x0201_C000;
const V_SCENE_FOLLOW_Y: u32 = 0x0300_3BBD;
/// Vertical offset of the scene (0x0801A3E8, an object of the level): the map is copied
/// that many rows lower (0x08025460).
const V_SCENE_DY: u32 = 0x0300_3DC6;

/// Scene 0x12, the clouds of World 1-5's sky area (0x154): its loader 0x08024620 unpacks
/// nothing; the level init 0x08016434 calls 0x080257BC -> 0x080245F0 instead, which unpacks
/// LZ77 0x0822123C (a whole 512x512 map) straight into BG2's screen block.
const SCENE_CLOUDS: u8 = 0x12;
const FN_CLOUD_MAP: u32 = 0x0802_45F0;
/// BG2CNT's shadow; the cloud map goes to a screen block clear of the BG tiles.
const V_BG2_CNT: u32 = 0x0300_2DA8;
const CLOUD_BG2_CNT: u16 = 0xDC02;

fn background(rom: &Rom, m: &mut Machine, scene: u8, mode: u8) -> Result<Option<Background>> {
    // Scene 0 has no data; scene mode 3 (the airship's sky) is drawn from `castle::extract_sky`.
    if rom.u32(SCENE_DATA + scene as u32 * 4)? == 0 || mode == AIRSHIP_SKY_MODE {
        return Ok(None);
    }
    let data = rom.ptr(SCENE_DATA + scene as u32 * 4)?;
    m.w8(V_SCENE_FOLLOW_Y, rom.u8(data + 1)?)?;
    m.call(rom.u32(SCENE_LOADERS + scene as u32 * 4)?, &[])?;
    let dy = m.read(V_SCENE_DY, 2)?;
    if dy != [0, 0] {
        // TODO(re): the shifted copy of 0x08025460 (not needed in World 1).
        println!("  background scene {scene}: offset {dy:02x?} not ported, left out");
        return Ok(None);
    }
    let follow_y = m.read(V_SCENE_FOLLOW_Y, 1)?[0] != 0;
    // 0x080245B4 (some scene loaders): BG2 priority 3, BG3 2.
    let far_front = m.read(V_BG2_CNT, 1)?[0] & 3 == 3;
    let map = if scene == SCENE_CLOUDS {
        let cnt = m.read(V_BG2_CNT, 2)?;
        m.w16(V_BG2_CNT, CLOUD_BG2_CNT)?;
        m.call(FN_CLOUD_MAP, &[])?;
        m.write(V_BG2_CNT, &cnt)?;
        m.read(0x0600_0000 + (CLOUD_BG2_CNT as u32 & 0x1F00) * 8, 0x2000)?
    } else {
        m.read(SCENE_MAP, 0x2000)?
    };
    let chars = m.read(0x0600_0000, 0x8000)?;
    let palette = parse_palette(&m.read(PAL_BUFFER, 0x200)?);
    let far = far_background(rom, m, mode, &palette)?.map(|image| FarBackground { front: far_front, image });
    // 512x512 text BG: four 32x32 screen blocks, top-left, top-right, bottom-left,
    // bottom-right.
    let w = BACKGROUND_PX as usize;
    let mut px = vec![0u8; w * w * 4];
    for (i, e) in map.chunks_exact(2).enumerate() {
        let e = sma4_rom::gfx::MapEntry::from(u16::from_le_bytes([e[0], e[1]]));
        let (block, cell) = (i / 0x400, i % 0x400);
        let tx = (block % 2) * 32 + cell % 32;
        let ty = (block / 2) * 32 + cell / 32;
        let tile = &chars[(e.tile as usize * 32).min(chars.len() - 32)..][..32];
        for y in 0..8 {
            for x in 0..8 {
                let sx = if e.hflip { 7 - x } else { x };
                let sy = if e.vflip { 7 - y } else { y };
                let b = tile[sy * 4 + sx / 2];
                let ci = if sx & 1 == 1 { b >> 4 } else { b & 0xF };
                if ci != 0 {
                    let o = ((ty * 8 + y) * w + tx * 8 + x) * 4;
                    px[o..o + 4].copy_from_slice(
                        &palette[e.palette as usize * 16 + ci as usize].to_rgba(255),
                    );
                }
            }
        }
    }
    Ok(Some(Background {
        scene,
        follow_y,
        mode,
        far,
        image: Rgba {
            width: BACKGROUND_PX,
            height: BACKGROUND_PX,
            pixels: px,
        },
    }))
}

/// Scene-mode routines (0x08016434 calls 0x081378AC[0x03003B9A]); some unpack a map into
/// 0x0201C000 and copy it to BG3's screen block (0x080257DC).
const SCENE_MODES: u32 = 0x0813_78AC;
/// The modes whose BG3 scroll the port follows (VBlank 0x0800092C): 2 (World 1-1's
/// clouds) and 9, 0xA, 0xF (the fortress's and World 1-5's arches).
const FAR_MODES: [u8; 4] = [2, 9, 0xA, 0xF];
const V_BG3_CNT: u32 = 0x0300_2DAA;
/// BG3's tiles: char block 2 (BG3CNT 0x960B).
const FAR_CHARS: u32 = 0x0600_8000;

/// BG3 as the scene mode's routine leaves it: 32x64 map entries (two screen blocks, one
/// above the other) over char block 2, colour 0 transparent.
fn far_background(rom: &Rom, m: &mut Machine, mode: u8, palette: &[sma4_rom::gfx::Bgr555]) -> Result<Option<Rgba>> {
    if !FAR_MODES.contains(&mode) {
        return Ok(None);
    }
    m.call(rom.u32(SCENE_MODES + mode as u32 * 4)?, &[])?;
    let cnt = u16::from_le_bytes(m.read(V_BG3_CNT, 2)?.try_into().unwrap());
    let map = m.read(0x0600_0000 + (cnt as u32 & 0x1F00) * 8, 0x1000)?;
    let chars = m.read(FAR_CHARS, 0x8000)?;
    let (w, h) = (FAR_BACKGROUND_W as usize, FAR_BACKGROUND_H as usize);
    let mut px = vec![0u8; w * h * 4];
    for (i, e) in map.chunks_exact(2).enumerate() {
        let e = sma4_rom::gfx::MapEntry::from(u16::from_le_bytes([e[0], e[1]]));
        let (tx, ty) = (i % 32, i / 32);
        let tile = &chars[(e.tile as usize * 32).min(chars.len() - 32)..][..32];
        for y in 0..8 {
            for x in 0..8 {
                let sx = if e.hflip { 7 - x } else { x };
                let sy = if e.vflip { 7 - y } else { y };
                let b = tile[sy * 4 + sx / 2];
                let ci = if sx & 1 == 1 { b >> 4 } else { b & 0xF };
                if ci != 0 {
                    let o = ((ty * 8 + y) * w + tx * 8 + x) * 4;
                    px[o..o + 4].copy_from_slice(&palette[e.palette as usize * 16 + ci as usize].to_rgba(255));
                }
            }
        }
    }
    Ok(Some(Rgba { width: w as u32, height: h as u32, pixels: px }))
}

/// End rooms per world (transition 4, 0x080056A0).
const END_LEVELS: u32 = 0x0813_1EDA;
const END_LISTS: u32 = 0x0813_1EEA;
const END_TILESETS: u32 = 0x0813_1EFA;

pub fn end_rooms(rom: &Rom) -> Result<[sma4_assets::EndRoom; sma4_assets::WORLDS]> {
    let mut out = [sma4_assets::EndRoom::default(); sma4_assets::WORLDS];
    for (w, r) in out.iter_mut().enumerate() {
        let w = w as u32;
        *r = sma4_assets::EndRoom {
            level: rom.u16(END_LEVELS + w * 2)?,
            enemies: rom.u16(END_LISTS + w * 2)?,
            tileset: rom.u8(END_TILESETS + w)?,
        };
    }
    Ok(out)
}

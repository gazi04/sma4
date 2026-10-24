//! On-disk formats for assets extracted from the user's ROM by `sma4-extract`.
//!
//! Layout of the asset directory (default `assets/extracted/`, git-ignored, never shared):
//! ```text
//! manifest.txt              which ROM the assets came from
//! levels/level_XXX.bin      level layout (metatile ids), solidity, backdrop (XXX = level id, hex)
//! levels/level_XXX.png      all 512 metatiles of the level's tileset, 16x16 each, stacked
//! objects/coin_spin.png     spin frames of a coin popping out of a block
//! map/world1_walk.bin       world map grid, start, enterable spots; world1_meta.png / _mario.png
//! gfx/XXXXXXXX.png          every LZ77 tile blob in the ROM (greyscale), for the viewer
//! ```
//! The game reads only these files; it never opens the ROM.

use std::path::{Path, PathBuf};

pub const METATILE: u32 = 16;
pub const DEFAULT_DIR: &str = "assets/extracted";

const LEVEL_MAGIC: &[u8; 8] = b"SMA4LV15";

#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("{0}: not a level asset (re-run sma4-extract)")]
    BadLevel(PathBuf),
    #[error("{path}: bad PNG: {msg}")]
    Png { path: PathBuf, msg: String },
}

pub type Result<T> = std::result::Result<T, AssetError>;

fn read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|source| AssetError::Io { path: path.into(), source })
}

fn write(path: &Path, data: &[u8]) -> Result<()> {
    let io = |source| AssetError::Io { path: path.into(), source };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    std::fs::write(path, data).map_err(io)
}

/// How the player collides with a metatile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum TileKind {
    #[default]
    Empty = 0,
    /// Can be stood on; passable from below and the sides.
    Top = 1,
    Solid = 2,
}

impl TileKind {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Top,
            2 => Self::Solid,
            _ => Self::Empty,
        }
    }
}

/// 8-bit RGBA image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Rgba {
    pub fn write_png(&self, path: &Path) -> Result<()> {
        let png_err = |e: png::EncodingError| AssetError::Png { path: path.into(), msg: e.to_string() };
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, self.width, self.height);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().map_err(png_err)?;
            w.write_image_data(&self.pixels).map_err(png_err)?;
        }
        write(path, &out)
    }

    pub fn read_png(path: &Path) -> Result<Self> {
        let png_err = |msg: String| AssetError::Png { path: path.into(), msg };
        let data = read(path)?;
        let mut dec = png::Decoder::new(std::io::Cursor::new(data));
        dec.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
        let mut reader = dec.read_info().map_err(|e| png_err(e.to_string()))?;
        let mut buf = vec![0; reader.output_buffer_size().ok_or_else(|| png_err("image too large".into()))?];
        let info = reader.next_frame(&mut buf).map_err(|e| png_err(e.to_string()))?;
        if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
            return Err(png_err(format!("expected 8-bit RGBA, got {:?}", info.color_type)));
        }
        buf.truncate(info.buffer_size());
        Ok(Self { width: info.width, height: info.height, pixels: buf })
    }
}

/// Second-plane metatiles below this low byte collide like another id (0x080A06E8).
pub const HIGH_IDS: usize = 0x26;

/// SMA4/SMB3 solidity rule: within each quadrant (low byte >> 6), ids at or above
/// `solid[q]` are solid and ids at or above `top[q]` can be stood on. Metatiles of the
/// second plane (id | 0x100, the 0x4000 plane of the level RAM) with a low byte below
/// `HIGH_IDS` collide like `high[low]` (0x080A06E8: 0x0813E174[tileset]); the others run
/// special handlers (slopes and such, 0x0817315C) not ported yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Solidity {
    pub top: [u8; 4],
    pub solid: [u8; 4],
    pub high: [u8; HIGH_IDS],
}

impl Default for Solidity {
    fn default() -> Self {
        Self { top: [0; 4], solid: [0; 4], high: [0; HIGH_IDS] }
    }
}

impl Solidity {
    /// The id a metatile collides as; `None` for second-plane ids with special handlers.
    pub fn id(&self, metatile: u16) -> Option<u8> {
        if metatile >= 0x100 { self.high.get(metatile as u8 as usize).copied() } else { Some(metatile as u8) }
    }

    pub fn kind(&self, metatile: u16) -> TileKind {
        let Some(id) = self.id(metatile) else {
            return TileKind::Empty;
        };
        let q = (id >> 6) as usize;
        if id >= self.solid[q] {
            TileKind::Solid
        } else if id >= self.top[q] {
            TileKind::Top
        } else {
            TileKind::Empty
        }
    }
}

/// Number of slope shapes (0x00-0x14).
pub const SLOPE_SHAPES: usize = 0x15;

/// Slope collision of tilesets with slopes (hills 3, underground 14: 0x080A1CEC sets
/// 0x03003CF4), read from the ROM by the extractor (see docs/re-notes.md, "Slopes").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slopes {
    /// Shape per collision id: 0x08169E68[id >> 6][id - top threshold] (0x080A3760), 0 below
    /// the threshold.
    pub shape: [u8; 256],
    /// Per shape, 16 columns (0x0816A029): low nibble = floor offset in the tile (0x10 =
    /// none), high nibble = ceiling.
    pub height: [[u8; 16]; SLOPE_SHAPES],
    /// Shapes without a floor (0x08172DE3 nonzero: empty and ceiling slopes).
    pub no_floor: [bool; SLOPE_SHAPES],
    /// Push down the slope per shape (0x0816A179), in speed units.
    pub push: [i8; SLOPE_SHAPES],
}

impl Default for Slopes {
    fn default() -> Self {
        Self { shape: [0; 256], height: [[0x10; 16]; SLOPE_SHAPES], no_floor: [true; SLOPE_SHAPES], push: [0; SLOPE_SHAPES] }
    }
}

impl Slopes {
    const BYTES: usize = 256 + SLOPE_SHAPES * 16 + SLOPE_SHAPES * 2;

    /// Shape of a metatile; second-plane ids collide like `Solidity::high` (no slope for
    /// the special handlers).
    pub fn shape_of(&self, solidity: &Solidity, metatile: u16) -> u8 {
        let id = if metatile >= 0x100 {
            match solidity.high.get(metatile as u8 as usize) {
                Some(&id) => id,
                None => return 0,
            }
        } else {
            metatile as u8
        };
        self.shape[id as usize]
    }

    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.shape);
        self.height.iter().for_each(|h| out.extend_from_slice(h));
        out.extend(self.no_floor.iter().map(|&b| b as u8));
        out.extend(self.push.iter().map(|&b| b as u8));
    }

    fn read(d: &[u8]) -> Option<Self> {
        let d = d.get(..Self::BYTES)?;
        let (shape, rest) = d.split_at(256);
        let (height, rest) = rest.split_at(SLOPE_SHAPES * 16);
        let (no_floor, push) = rest.split_at(SLOPE_SHAPES);
        Some(Self {
            shape: shape.try_into().ok()?,
            height: std::array::from_fn(|i| height[i * 16..i * 16 + 16].try_into().unwrap()),
            no_floor: std::array::from_fn(|i| no_floor[i] != 0),
            push: std::array::from_fn(|i| push[i] as i8),
        })
    }
}

/// Size of a level background (BG2, 64x64 tiles: 0xDC02 in the level's BG CNT setup).
pub const BACKGROUND_PX: u32 = 512;

/// The level's background scene (header byte 10, loader 0x08025460): a 512x512 image drawn
/// on BG2 behind the level, colour 0 transparent (the backdrop shows through).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Background {
    pub scene: u8,
    /// The scene's flag 0x08139A2C[scene][1] (0x03003BBD): the background scrolls with the
    /// camera vertically instead of at 3/4 speed.
    pub follow_y: bool,
    pub image: Rgba,
}

/// Block-hit rules of a tileset, read from the ROM by the extractor (see docs/re-notes.md,
/// "Blocks and items"). The game logic that uses them lives in `sma4-core`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockRules {
    /// Per low-layer metatile id: behaviour when hit by the player's head-centre probe while
    /// moving up, or `NOT_BUMPABLE`.
    pub bump_behaviour: [u8; 256],
    /// Per low-layer metatile id: behaviour when the feet probe (probe 0) stands on it
    /// (note blocks), or `NOT_BUMPABLE`.
    pub land_behaviour: [u8; 256],
    /// Per metatile id: high nibble of the hit result (selects the final tile kind).
    pub result_hi: [u8; 256],
    /// Object type spawned for a hit result's low nibble (0 = none).
    pub item_types: [u16; 16],
    /// Final tile kind for a hit result's high nibble.
    pub final_kind: [u8; 16],
    /// Metatile written when a bump finishes, indexed by `kind - 1`.
    pub final_metatile: [u16; 32],
    /// Bump-block vertical speed (1/16 px per frame) indexed by the bump timer (10 -> 1).
    pub bounce: [i8; 11],
}

impl BlockRules {
    pub const NOT_BUMPABLE: u8 = 0xFF;
    const BYTES: usize = 256 + 256 + 256 + 32 + 16 + 64 + 11;

    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.bump_behaviour);
        out.extend_from_slice(&self.land_behaviour);
        out.extend_from_slice(&self.result_hi);
        self.item_types.iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
        out.extend_from_slice(&self.final_kind);
        self.final_metatile.iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
        out.extend(self.bounce.iter().map(|&b| b as u8));
    }

    fn read(d: &[u8]) -> Self {
        let u16s = |d: &[u8]| -> Vec<u16> { d.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect() };
        let mut p = 0;
        let mut take = |n: usize| {
            let s = &d[p..p + n];
            p += n;
            s
        };
        Self {
            bump_behaviour: take(256).try_into().unwrap(),
            land_behaviour: take(256).try_into().unwrap(),
            result_hi: take(256).try_into().unwrap(),
            item_types: u16s(take(32)).try_into().unwrap(),
            final_kind: take(16).try_into().unwrap(),
            final_metatile: u16s(take(64)).try_into().unwrap(),
            bounce: take(11).iter().map(|&b| b as i8).collect::<Vec<_>>().try_into().unwrap(),
        }
    }
}

impl Default for BlockRules {
    fn default() -> Self {
        Self {
            bump_behaviour: [Self::NOT_BUMPABLE; 256],
            land_behaviour: [Self::NOT_BUMPABLE; 256],
            result_hi: [0; 256],
            item_types: [0; 16],
            final_kind: [0; 16],
            final_metatile: [0; 32],
            bounce: [0; 11],
        }
    }
}

/// One entry of a level's enemy list (ROM table 0x08136B14): object type, the metatile
/// column/row it stands in and the record's extra bytes (0 when it has none), in list order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EnemySpawn {
    pub kind: u16,
    pub col: u16,
    pub row: u8,
    pub extra: [u8; 2],
}

/// Where a pipe of this area leads (level header bytes 0-3 and 6, loader 0x08014F20; the
/// switch itself is 0x08005690 -> 0x0800515C): the other area of the level, and the
/// player's position there by the screen page the pipe was entered on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Junction {
    pub level: u16,
    pub enemies: u16,
    pub tileset: u8,
    pub exits: Vec<PipeExit>,
}

/// One pointer record (object stream kind 0x80, stored by 0x08014CE0 at 0x0300381E /
/// 0x0300382E indexed by the record's row byte = the page), decoded by 0x08005290.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PipeExit {
    /// Screen page (x >> 8) the player enters the pipe on.
    pub page: u8,
    /// Top-left of the player's 16x32 frame in the other area, px.
    pub x: u16,
    pub y: u16,
    /// How the player comes out (0x03003BF1): 1 up out of a pipe, 2 down, 3 right, 4 left.
    pub dir: u8,
    /// Camera y set on arrival (0x03003754).
    pub camera_y: u16,
}

/// Metatiles per tileset: 256 ids, plus 256 more on the high layer.
pub const METATILES: u32 = 512;

/// Background tile animation frames (coins and question blocks spin): the game swaps
/// four versions of their tiles into VRAM, one every 8 frames (0x080016DC).
pub const ANIM_FRAMES: u32 = 4;

/// Animation frame shown on game frame `frame`: (frame & 0x18) >> 3.
pub const fn anim_frame(frame: u32) -> usize {
    ((frame & 0x18) >> 3) as usize
}

/// A piece of a broken brick (effect kind 2): one 8x8 image.
pub fn brick_debris_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("brick_debris.png")
}

/// Sparkle left where a coin was collected: 8x8 cells, tile 0x68 + i (smallest first).
pub fn sparkle_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("sparkle.png")
}

/// A level ready to draw and play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelAsset {
    pub level_id: u16,
    pub tileset: u8,
    /// Size in metatiles.
    pub width: usize,
    pub height: usize,
    pub backdrop: [u8; 3],
    pub solidity: Solidity,
    /// Metatile per cell, row-major from the top-left: id | 0x100 for the high layer.
    pub metatiles: Vec<u16>,
    pub rules: BlockRules,
    pub enemies: Vec<EnemySpawn>,
    /// Time limit in seconds of the status-bar clock (0 = no clock): 100 x
    /// 0x08136016[header byte 8 >> 6] (level loader 0x08014F20).
    pub time_limit: u16,
    /// Player start y (top of the 32 px frame, px): 0x0813789A[header byte 4 >> 5]
    /// (0x08014F20).
    pub start_y: u16,
    /// Camera y at the start (0x03003754 = 0x0813600E[header byte 4 >> 5], 0x08014F20).
    pub start_camera_y: u16,
    /// Pipe flags of the tileset's class (0x08172E44[0x08172BCC[tileset]], 0x0809B598): bit
    /// 7 makes 0xBD / 0xBE pipe ends lead to the world's end room.
    pub pipe_flags: u8,
    /// Water: per id quadrant (id >> 6), the ids above this one are water unless they
    /// collide (0x0813DD04[class * 4 + quadrant], 0x080A1E60).
    pub water: [u8; 4],
    /// The vertical scroll's settings (0x08136F30[level id], loaded by 0x08014F20).
    pub camera: CameraParams,
    /// The pipe destination area, if the header names one.
    pub junction: Option<Junction>,
    /// Horizontal camera limits (min, max x of the view), when the ROM sets them for this
    /// area (0x080A4D4C, by object-stream address); otherwise the level's width.
    pub camera_x: Option<(u16, u16)>,
    /// Slope collision, for tilesets with slopes.
    pub slopes: Option<Slopes>,
    pub background: Option<Background>,
    /// All `METATILES` metatiles of the tileset rendered at 16x16, stacked vertically, once
    /// per animation frame: metatile `m` of frame `f` is rows `16*(f*METATILES+m)` + 0..16.
    pub atlas: Rgba,
}

impl LevelAsset {
    pub fn background_path(dir: &Path, level_id: u16) -> PathBuf {
        dir.join("levels").join(format!("level_{level_id:03x}_bg.png"))
    }

    pub fn paths(dir: &Path, level_id: u16) -> (PathBuf, PathBuf) {
        let stem = dir.join("levels").join(format!("level_{level_id:03x}"));
        (stem.with_extension("bin"), stem.with_extension("png"))
    }

    pub fn metatile(&self, x: usize, y: usize) -> u16 {
        self.metatiles[y * self.width + x]
    }

    pub fn kind(&self, x: usize, y: usize) -> TileKind {
        self.solidity.kind(self.metatile(x, y))
    }

    pub fn write(&self, dir: &Path) -> Result<()> {
        let (bin, png) = Self::paths(dir, self.level_id);
        let mut out = Vec::with_capacity(32 + self.metatiles.len() * 2);
        out.extend_from_slice(LEVEL_MAGIC);
        out.extend_from_slice(&self.level_id.to_le_bytes());
        out.extend_from_slice(&(self.width as u16).to_le_bytes());
        out.extend_from_slice(&(self.height as u16).to_le_bytes());
        out.extend_from_slice(&self.backdrop);
        out.push(self.tileset);
        out.extend_from_slice(&self.solidity.top);
        out.extend_from_slice(&self.solidity.solid);
        for m in &self.metatiles {
            out.extend_from_slice(&m.to_le_bytes());
        }
        self.rules.write(&mut out);
        out.extend_from_slice(&(self.enemies.len() as u16).to_le_bytes());
        for e in &self.enemies {
            out.extend_from_slice(&e.kind.to_le_bytes());
            out.extend_from_slice(&e.col.to_le_bytes());
            out.push(e.row);
            out.extend_from_slice(&e.extra);
        }
        out.extend_from_slice(&self.time_limit.to_le_bytes());
        out.extend_from_slice(&self.start_y.to_le_bytes());
        out.extend_from_slice(&self.solidity.high);
        match &self.junction {
            None => out.push(0),
            Some(j) => {
                out.push(1);
                out.extend_from_slice(&j.level.to_le_bytes());
                out.extend_from_slice(&j.enemies.to_le_bytes());
                out.push(j.tileset);
                out.push(j.exits.len() as u8);
                for e in &j.exits {
                    out.push(e.page);
                    out.extend_from_slice(&e.x.to_le_bytes());
                    out.extend_from_slice(&e.y.to_le_bytes());
                    out.push(e.dir);
                    out.extend_from_slice(&e.camera_y.to_le_bytes());
                }
            }
        }
        match self.camera_x {
            None => out.push(0),
            Some((min, max)) => {
                out.push(1);
                out.extend_from_slice(&min.to_le_bytes());
                out.extend_from_slice(&max.to_le_bytes());
            }
        }
        match &self.slopes {
            None => out.push(0),
            Some(s) => {
                out.push(1);
                s.write(&mut out);
            }
        }
        match &self.background {
            None => out.push(0),
            Some(b) => {
                out.extend_from_slice(&[1, b.scene, b.follow_y as u8]);
                b.image.write_png(&Self::background_path(dir, self.level_id))?;
            }
        }
        out.extend_from_slice(&self.start_camera_y.to_le_bytes());
        out.push(self.pipe_flags);
        out.extend_from_slice(&self.water);
        self.camera.write(&mut out);
        write(&bin, &out)?;
        self.atlas.write_png(&png)
    }

    pub fn read(dir: &Path, level_id: u16) -> Result<Self> {
        let (bin, png) = Self::paths(dir, level_id);
        let d = read(&bin)?;
        let bad = || AssetError::BadLevel(bin.clone());
        const HEADER: usize = 26;
        if d.len() < HEADER || &d[..8] != LEVEL_MAGIC {
            return Err(bad());
        }
        let u16_at = |o: usize| u16::from_le_bytes([d[o], d[o + 1]]);
        let (width, height) = (u16_at(10) as usize, u16_at(12) as usize);
        let body = &d[HEADER..];
        let cells = width * height * 2;
        let enemies_at = cells + BlockRules::BYTES;
        if body.len() < enemies_at + 2 {
            return Err(bad());
        }
        let metatiles = body[..cells].chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
        let rules = BlockRules::read(&body[cells..enemies_at]);
        let count = u16::from_le_bytes([body[enemies_at], body[enemies_at + 1]]) as usize;
        let list = &body[enemies_at + 2..];
        let n = count * ENEMY_BYTES;
        if list.len() < n + 4 + HIGH_IDS + 2 {
            return Err(bad());
        }
        let high_at = n + 4;
        let time_limit = u16::from_le_bytes([list[n], list[n + 1]]);
        let start_y = u16::from_le_bytes([list[n + 2], list[n + 3]]);
        let enemies = list[..n]
            .chunks_exact(ENEMY_BYTES)
            .map(|e| EnemySpawn {
                kind: u16::from_le_bytes([e[0], e[1]]),
                col: u16::from_le_bytes([e[2], e[3]]),
                row: e[4],
                extra: [e[5], e[6]],
            })
            .collect();
        let (junction, camera_x, slopes, scene, start_camera_y, pipe_flags, water, camera) =
            read_area(&list[high_at + HIGH_IDS..]).ok_or_else(bad)?;
        let background = match scene {
            None => None,
            Some((scene, follow_y)) => {
                Some(Background { scene, follow_y, image: Rgba::read_png(&Self::background_path(dir, level_id))? })
            }
        };
        let atlas = Rgba::read_png(&png)?;
        if atlas.width != METATILE || atlas.height != METATILE * METATILES * ANIM_FRAMES {
            return Err(AssetError::Png { path: png, msg: "atlas must be 16 x 32768 (re-run sma4-extract)".into() });
        }
        Ok(Self {
            level_id: u16_at(8),
            tileset: d[17],
            width,
            height,
            backdrop: [d[14], d[15], d[16]],
            solidity: Solidity {
                top: [d[18], d[19], d[20], d[21]],
                solid: [d[22], d[23], d[24], d[25]],
                high: list[high_at..high_at + HIGH_IDS].try_into().map_err(|_| bad())?,
            },
            metatiles,
            rules,
            enemies,
            time_limit,
            start_y,
            start_camera_y,
            pipe_flags,
            water,
            camera,
            junction,
            camera_x,
            slopes,
            background,
            atlas,
        })
    }
}

/// A level's vertical scroll settings (u16s of 0x08136F30[level id], 0x08014F20).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CameraParams {
    /// +0: the level's last pixel row (player +0xCA); the camera stops at this - 0x9F.
    pub bottom: u16,
    /// +2: bit 15 set, a fixed camera y (player +0xC8).
    pub fixed: u16,
    /// +4 / +6 (+0xD0 / +0xD4) and +0x16 / +0x18 (+0xD2 / +0xD6): the dead zone (player y
    /// less the camera y) the camera does not follow inside, normally and while flying.
    pub dead_zone: [u16; 2],
    pub dead_zone_flying: [u16; 2],
    /// +0x1E (+0xDA): the highest camera y.
    pub top: u16,
}

impl CameraParams {
    fn write(&self, out: &mut Vec<u8>) {
        let [d0, d1] = self.dead_zone;
        let [f0, f1] = self.dead_zone_flying;
        for v in [self.bottom, self.fixed, d0, d1, f0, f1, self.top] {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }

    fn read(d: &[u8]) -> Option<Self> {
        let w = |i: usize| Some(u16::from_le_bytes([*d.get(i * 2)?, *d.get(i * 2 + 1)?]));
        Some(Self {
            bottom: w(0)?,
            fixed: w(1)?,
            dead_zone: [w(2)?, w(3)?],
            dead_zone_flying: [w(4)?, w(5)?],
            top: w(6)?,
        })
    }
}

const EXIT_BYTES: usize = 8;
/// An enemy-list entry in the level file: kind, column, row, two extra bytes.
const ENEMY_BYTES: usize = 7;

/// The level file's tail: the junction, the camera limits, the slopes, the background
/// scene (id, follows y), the start camera y, the pipe flags and the water thresholds.
type Area =
    (Option<Junction>, Option<(u16, u16)>, Option<Slopes>, Option<(u8, bool)>, u16, u8, [u8; 4], CameraParams);

fn read_area(d: &[u8]) -> Option<Area> {
    let u16_at = |o: usize| Some(u16::from_le_bytes([*d.get(o)?, *d.get(o + 1)?]));
    let mut p = 1;
    let junction = match *d.first()? {
        0 => None,
        _ => {
            let n = *d.get(6)? as usize;
            let exits = (0..n)
                .map(|i| {
                    let o = 7 + i * EXIT_BYTES;
                    Some(PipeExit {
                        page: *d.get(o)?,
                        x: u16_at(o + 1)?,
                        y: u16_at(o + 3)?,
                        dir: *d.get(o + 5)?,
                        camera_y: u16_at(o + 6)?,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            p = 7 + n * EXIT_BYTES;
            Some(Junction { level: u16_at(1)?, enemies: u16_at(3)?, tileset: *d.get(5)?, exits })
        }
    };
    let camera_x = match *d.get(p)? {
        0 => None,
        _ => Some((u16_at(p + 1)?, u16_at(p + 3)?)),
    };
    p += if camera_x.is_some() { 5 } else { 1 };
    let slopes = match *d.get(p)? {
        0 => None,
        _ => Some(Slopes::read(d.get(p + 1..)?)?),
    };
    p += if slopes.is_some() { 1 + Slopes::BYTES } else { 1 };
    let scene = match *d.get(p)? {
        0 => None,
        _ => Some((*d.get(p + 1)?, *d.get(p + 2)? != 0)),
    };
    p += if scene.is_some() { 3 } else { 1 };
    let camera = CameraParams::read(d.get(p + 7..)?)?;
    Some((junction, camera_x, slopes, scene, u16_at(p)?, *d.get(p + 2)?, d.get(p + 3..p + 7)?.try_into().ok()?, camera))
}

/// Status bar (BG0 in levels): the template map the level loader 0x08016434 decompresses
/// (LZ77 0x0821F788), plus the ROM tables the update routines write from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HudAsset {
    /// `HUD_COLS` x `HUD_ROWS` BG map entries (tile | flips | palette << 12).
    pub template: Vec<u16>,
    /// Digit tiles 0-9 (0x08131E6C).
    pub digits: [u16; 10],
    /// Character icon, two tiles (0x08131E64: Mario).
    pub icon: [u16; 2],
}

pub const HUD_COLS: usize = 32;
pub const HUD_ROWS: usize = 20;
/// Status-bar tiles in `hud/status_tiles.png`: 8x8 cells for tiles `HUD_TILE_BASE..+128`,
/// first in BG palette 0, then the same tiles in palette 15.
pub const HUD_TILE_BASE: u16 = 0x280;
pub const HUD_TILES: usize = 128;

/// Cell of a status-bar map entry in `hud/status_tiles.png`, or None for an empty cell.
pub fn hud_cell(entry: u16) -> Option<usize> {
    let tile = entry & 0x3FF;
    let pal = entry >> 12;
    if !(HUD_TILE_BASE..HUD_TILE_BASE + HUD_TILES as u16).contains(&tile) || tile == 0x2FF {
        return None;
    }
    let base = match pal {
        0 => 0,
        15 => HUD_TILES,
        _ => return None,
    };
    Some(base + (tile - HUD_TILE_BASE) as usize)
}

impl HudAsset {
    pub fn path(dir: &Path) -> PathBuf {
        dir.join("hud").join("status_map.bin")
    }

    pub fn tiles_path(dir: &Path) -> PathBuf {
        dir.join("hud").join("status_tiles.png")
    }

    pub fn write(&self, dir: &Path) -> Result<()> {
        let words = self.template.iter().chain(&self.digits).chain(&self.icon);
        let out: Vec<u8> = words.flat_map(|w| w.to_le_bytes()).collect();
        write(&Self::path(dir), &out)
    }

    pub fn read(dir: &Path) -> Result<Self> {
        let path = Self::path(dir);
        let d = read(&path)?;
        let n = HUD_COLS * HUD_ROWS;
        if d.len() != (n + 12) * 2 {
            return Err(AssetError::BadLevel(path));
        }
        let w: Vec<u16> = d.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
        Ok(Self {
            template: w[..n].to_vec(),
            digits: w[n..n + 10].try_into().unwrap(),
            icon: [w[n + 10], w[n + 11]],
        })
    }
}

/// A spot of the world map the level tables know (0x08133A88 / AAC / AD0 / AF4 for the
/// world): map position, tileset and the level / enemy-list ids (bit 15 set: not a level,
/// e.g. a Toad house item or the spade game).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapNode {
    /// Map y (row = (y - 0x10) >> 4) and x (screen << 8 | x; column = x >> 4).
    pub y: u8,
    pub x: u16,
    pub tileset: u8,
    pub level: u16,
    pub enemies: u16,
}

/// Map tile animation frames (VBlank 0x080016DC: frame counter bits 3-4).
pub const MAP_ANIM_FRAMES: u32 = 4;
/// Map sprite cells of Mario in `world{n}_mario.png`: 16x32 each, (form 0 small / 1 big /
/// 2 raccoon) x (two animation phases); the lower OBJ fills the bottom 16 rows.
pub const MAP_MARIO_W: u32 = 16;
pub const MAP_MARIO_H: u32 = 32;
pub const MAP_MARIO_FORMS: u32 = 3;
/// Mario spinning on the map after a game over (0x0800C45C while map state +0x17 is set):
/// the lower OBJ becomes 0x0813A55F[(+0x21 & 6) >> 1] (facing left, front, facing right, back),
/// 16x16 cells in `world{n}_mario_spin.png`, in that table's order.
pub const MAP_SPIN_SIZE: u32 = 16;
pub const MAP_SPIN_PHASES: u32 = 4;

/// The interactive world map: the tile grid (map tile ids, one per 16x16 cell), where
/// Mario starts, the enterable spots, and the metatile pictures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldMapAsset {
    pub cols: usize,
    pub rows: usize,
    pub grid: Vec<u8>,
    /// Start position (map y, x).
    pub start: (u8, u16),
    /// Enterable-tile thresholds 0x030037D6[id >> 6] (tiles at or above can be entered).
    pub thresholds: [u8; 4],
    pub nodes: Vec<MapNode>,
    /// Screen position of cell (0, 0) with the map unscrolled.
    pub origin: (i32, i32),
    /// Metatile pictures: 256 cells of 16x16 stacked down, one column per animation frame.
    pub atlas: Rgba,
    /// Mario's map sprite, see `MAP_MARIO_*`.
    pub mario: Rgba,
    /// Frames each animation frame is shown (0x08135C5E[world * 4 + frame] + 1).
    pub anim_ticks: [u8; 4],
    /// Completion bits a game over keeps, per column (0x08012458 ANDs 0x02001420[world *
    /// 0x80 + column] with 0x0813C12E[world * 0x40 + column]): the levels, fortresses and
    /// locks stay done, Toad houses and spade panels come back.
    pub game_over_keep: Vec<u8>,
    /// Mario spinning, see `MAP_SPIN_*` (empty when extracted by an older version).
    pub mario_spin: Rgba,
}

/// Version 1 lacks `game_over_keep` (read as "keep everything").
const MAP_MAGIC_V1: &[u8; 8] = b"SMA4WM01";
const MAP_MAGIC: &[u8; 8] = b"SMA4WM02";

impl WorldMapAsset {
    fn stem(dir: &Path, world: u8) -> PathBuf {
        dir.join("map").join(format!("world{}_walk", world + 1))
    }

    pub fn tile(&self, col: usize, row: usize) -> u8 {
        if col < self.cols && row < self.rows { self.grid[row * self.cols + col] } else { 0x02 }
    }

    pub fn write(&self, dir: &Path, world: u8) -> Result<()> {
        let stem = Self::stem(dir, world);
        let mut out = MAP_MAGIC.to_vec();
        for v in [self.cols as u16, self.rows as u16, self.start.0 as u16, self.start.1] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&self.thresholds);
        out.extend_from_slice(&self.anim_ticks);
        for v in [self.origin.0 as i16, self.origin.1 as i16] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&(self.nodes.len() as u16).to_le_bytes());
        for n in &self.nodes {
            out.push(n.y);
            out.push(n.tileset);
            for v in [n.x, n.level, n.enemies] {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out.extend_from_slice(&self.grid);
        out.extend_from_slice(&self.game_over_keep);
        write(&stem.with_extension("bin"), &out)?;
        self.atlas.write_png(&dir.join("map").join(format!("world{}_meta.png", world + 1)))?;
        self.mario.write_png(&dir.join("map").join(format!("world{}_mario.png", world + 1)))?;
        self.mario_spin.write_png(&dir.join("map").join(format!("world{}_mario_spin.png", world + 1)))
    }

    pub fn read(dir: &Path, world: u8) -> Result<Self> {
        let stem = Self::stem(dir, world);
        let path = stem.with_extension("bin");
        let d = read(&path)?;
        let bad = || AssetError::BadLevel(path.clone());
        if d.len() < 30 || (&d[..8] != MAP_MAGIC && &d[..8] != MAP_MAGIC_V1) {
            return Err(bad());
        }
        let u16_at = |i: usize| u16::from_le_bytes([d[i], d[i + 1]]);
        let (cols, rows) = (u16_at(8) as usize, u16_at(10) as usize);
        let start = (u16_at(12) as u8, u16_at(14));
        let thresholds = [d[16], d[17], d[18], d[19]];
        let anim_ticks = [d[20], d[21], d[22], d[23]];
        let origin = (u16_at(24) as i16 as i32, u16_at(26) as i16 as i32);
        let n = u16_at(28) as usize;
        let mut at = 30;
        let mut nodes = Vec::with_capacity(n);
        for _ in 0..n {
            let b = d.get(at..at + 8).ok_or_else(bad)?;
            let w = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
            nodes.push(MapNode { y: b[0], tileset: b[1], x: w(2), level: w(4), enemies: w(6) });
            at += 8;
        }
        let grid = d.get(at..at + cols * rows).ok_or_else(bad)?.to_vec();
        at += cols * rows;
        let game_over_keep =
            if &d[..8] == MAP_MAGIC { d.get(at..at + cols).ok_or_else(bad)?.to_vec() } else { vec![0xFF; cols] };
        let atlas = Rgba::read_png(&dir.join("map").join(format!("world{}_meta.png", world + 1)))?;
        let mario = Rgba::read_png(&dir.join("map").join(format!("world{}_mario.png", world + 1)))?;
        let mario_spin = Rgba::read_png(&dir.join("map").join(format!("world{}_mario_spin.png", world + 1)))
            .unwrap_or(Rgba { width: 0, height: 0, pixels: vec![] });
        Ok(Self { cols, rows, grid, start, thresholds, nodes, origin, atlas, mario, anim_ticks, game_over_keep, mario_spin })
    }
}

/// World map screen (see `sma4-extract` screens.rs): the 240x160 map view, the status
/// bar (BG0) and the "WORLD n / MARIO x n" panel (BG3) as 32x32 map entries with their
/// scroll, the tiles those entries (and the digits written into them) use, and the
/// panel's sprites as a 240x160 overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapScreenAsset {
    pub map: Rgba,
    pub bar: Vec<u16>,
    pub bar_scroll: (u16, u16),
    pub panel: Vec<u16>,
    pub panel_scroll: (u16, u16),
    /// Tile cells in `tiles`: (layer 0 = bar / 3 = panel, entry without flips).
    pub keys: Vec<(u8, u16)>,
    pub tiles: Rgba,
    pub sprites: Rgba,
}

impl MapScreenAsset {
    fn stem(dir: &Path, world: u8) -> PathBuf {
        dir.join("map").join(format!("world{}", world + 1))
    }

    /// Cell in `tiles` for a map entry of `layer`, or None for an empty cell.
    pub fn cell(&self, layer: u8, entry: u16) -> Option<usize> {
        self.keys.iter().position(|&k| k == (layer, entry & 0xF3FF))
    }

    pub fn write(&self, dir: &Path, world: u8) -> Result<()> {
        let stem = Self::stem(dir, world);
        let mut out = Vec::new();
        for &w in self.bar.iter().chain(&self.panel) {
            out.extend_from_slice(&w.to_le_bytes());
        }
        for v in [self.bar_scroll.0, self.bar_scroll.1, self.panel_scroll.0, self.panel_scroll.1] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&(self.keys.len() as u16).to_le_bytes());
        for &(layer, e) in &self.keys {
            out.push(layer);
            out.extend_from_slice(&e.to_le_bytes());
        }
        write(&stem.with_extension("bin"), &out)?;
        self.map.write_png(&stem.with_extension("png"))?;
        self.tiles.write_png(&dir.join("map").join(format!("world{}_tiles.png", world + 1)))?;
        self.sprites.write_png(&dir.join("map").join(format!("world{}_sprites.png", world + 1)))
    }

    pub fn read(dir: &Path, world: u8) -> Result<Self> {
        let stem = Self::stem(dir, world);
        let bin = stem.with_extension("bin");
        let d = read(&bin)?;
        let bad = || AssetError::BadLevel(bin.clone());
        let u16_at = |o: usize| d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
        let n = 32 * 32;
        let words: Vec<u16> = (0..2 * n + 5).map(|i| u16_at(i * 2)).collect::<Option<_>>().ok_or_else(bad)?;
        let count = words[2 * n + 4] as usize;
        let base = (2 * n + 5) * 2;
        let keys = (0..count)
            .map(|i| Some((*d.get(base + i * 3)?, u16_at(base + i * 3 + 1)?)))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(bad)?;
        Ok(Self {
            bar: words[..n].to_vec(),
            panel: words[n..2 * n].to_vec(),
            bar_scroll: (words[2 * n], words[2 * n + 1]),
            panel_scroll: (words[2 * n + 2], words[2 * n + 3]),
            keys,
            map: Rgba::read_png(&stem.with_extension("png"))?,
            tiles: Rgba::read_png(&dir.join("map").join(format!("world{}_tiles.png", world + 1)))?,
            sprites: Rgba::read_png(&dir.join("map").join(format!("world{}_sprites.png", world + 1)))?,
        })
    }
}

/// Game-over screen pieces: ten 8x16 letter cells (OAM slots 1-10), then the 16x16 Mario
/// head, in a 96x16 strip.
pub fn game_over_path(dir: &Path) -> PathBuf {
    dir.join("screens").join("game_over.png")
}

/// The game-over menu screen (dimmed map with the "GAME OVER / CONTINUE / QUIT" panel,
/// 240x160, without the status bar) and its 8x8 cursor.
pub fn game_over_menu_path(dir: &Path) -> PathBuf {
    dir.join("screens").join("game_over_menu.png")
}
pub fn game_over_cursor_path(dir: &Path) -> PathBuf {
    dir.join("screens").join("game_over_cursor.png")
}

/// Score popups (0x08093750): 16x8 cells for popup types 1-12 (10 ... 8000 points).
pub fn score_popups_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("score_popups.png")
}
pub const SCORE_POPUP_TYPES: usize = 12;

/// Extra-life popups (0x080938B4): 32x16 cells for types 13-15 (1UP, 2UP, 3UP).
pub fn life_popups_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("life_popups.png")
}
pub const LIFE_POPUP_TYPES: usize = 3;

/// Graphics sheets for the viewer, sorted by ROM address.
pub fn gfx_sheets(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir.join("gfx"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect();
    v.sort();
    v
}

/// Spin frames of a coin popping out of a block: 16x16 cells side by side.
pub fn coin_spin_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("coin_spin.png")
}

/// Basic walking enemy (object type 0x72): 16x16 cells, see `WALKER_FRAMES`.
pub fn walker_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("walker.png")
}

/// Walker frames in `walker.png`: two walk frames, then the squished frame.
pub const WALKER_FRAMES: u32 = 3;
pub const WALKER_SQUISHED: usize = 2;

/// Growth item (object type 0x0D, the mushroom): one 16x16 image.
pub fn growth_item_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("growth_item.png")
}

/// Koopa Troopa sheets (green 0x6C, red 0x6D, green Paratroopa 0x6E): cells of
/// `KOOPA_CELL_W` x `KOOPA_CELL_H`, the object's 16x16 box top-left at `KOOPA_ORIGIN`.
pub fn koopa_path(dir: &Path, kind: u16) -> PathBuf {
    dir.join("objects").join(format!("koopa_{kind:02x}.png"))
}
pub const KOOPA_CELL_W: u32 = 32;
pub const KOOPA_CELL_H: u32 = 48;
pub const KOOPA_ORIGIN: (i32, i32) = (8, 24);
/// Koopa cells: walk 0/1, shell, shell with legs out (about to wake), spinning shell 0-3.
/// Paratroopa sheets hold four winged walk cells instead (wing flap x walk step).
pub const KOOPA_WALK: usize = 0;
pub const KOOPA_SHELL: usize = 2;
pub const KOOPA_SHELL_WAKING: usize = 3;
pub const KOOPA_SPIN: usize = 4;

/// Paragoombas (red 0x73, brown Micro-Goomba dropper 0x74; draw 0x08053D88), one sheet
/// per type: `KOOPA_CELL_W` x `KOOPA_CELL_H` cells, the
/// 16x16 box top-left at `KOOPA_ORIGIN`. Cells 0-3: `paragoomba_cell` (body walk frame x
/// wings 0x108 / 0x107, two 8x16 OBJs 10 px up, at x - 2 and x + 10, the right one
/// h-flipped); then the Goomba it turns into when stomped, which keeps its palette
/// (+0x21): walk frames 0/1 (`PARAGOOMBA_GOOMBA`) and flattened (`PARAGOOMBA_SQUISHED`).
pub fn paragoomba_path(dir: &Path, kind: u16) -> PathBuf {
    dir.join("objects").join(format!("paragoomba_{kind:02x}.png"))
}
pub fn paragoomba_cell(body: usize, wings_up: bool) -> usize {
    body * 2 + wings_up as usize
}
pub const PARAGOOMBA_GOOMBA: usize = 4;
pub const PARAGOOMBA_SQUISHED: usize = 6;
pub const PARAGOOMBA_CELLS: u32 = 7;

/// Piranha Plant sheets (green 0xA0, red 0xA2): cells of `KOOPA_CELL_W` x `KOOPA_CELL_H`
/// (the oam renderer's cell), the object's position at `PIRANHA_ORIGIN`. Cells: mouth open,
/// mouth closed (the chomp), then the same two while less than 17 px out of the pipe
/// (the ROM swaps the stem tile then).
pub fn piranha_path(dir: &Path, kind: u16) -> PathBuf {
    dir.join("objects").join(format!("piranha_{kind:02x}.png"))
}
pub const PIRANHA_FRAMES: u32 = 4;
/// The plant reaches 33 px below its position, so it sits higher in the cell than a Koopa.
pub const PIRANHA_ORIGIN: (i32, i32) = (8, 8);
pub const PIRANHA_LOW: usize = 2;
/// The Venus Fire Trap 0xA4 (`piranha_path(dir, 0xA4)`) has the four cells for each way it
/// looks (0x0805564C: h-flip from +0x1A bit 6, v-flip while the player is above), in the
/// order `fire_piranha_cell` gives.
pub const FIRE_PIRANHA_LOOKS: u32 = 4;
pub fn fire_piranha_cell(right: bool, up: bool, frame: usize) -> usize {
    (up as usize * 2 + right as usize) * PIRANHA_FRAMES as usize + frame
}

/// Venus Fire Trap fireball (projectile type 5, draw 0x0808D5F0): `KOOPA_CELL_W` x
/// `KOOPA_CELL_H` cells, the projectile's position at `KOOPA_ORIGIN`. Spin frames 0-3
/// (tile 0x08170AEA[(counter >> 2) & 3], frames 2 and 3 flipped both ways) flying right,
/// then the same flying left.
pub fn fireball_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("fireball.png")
}
pub const FIREBALL_FRAMES: u32 = 4;

/// Micro-Goomba (projectile type 6, draw 0x0808D7E0): `KOOPA_CELL_W` x `KOOPA_CELL_H`
/// cells, the projectile's position (top-left of its 8x8 OBJ) at `KOOPA_ORIGIN`. Cells:
/// falling or stuck (`MICRO_GOOMBA_FLIP` h-flipped, the ROM flips it every 16 frames),
/// then shaken off (`MICRO_GOOMBA_OFF`: upside down, palette 3).
pub fn micro_goomba_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("micro_goomba.png")
}
pub const MICRO_GOOMBA_FLIP: usize = 1;
pub const MICRO_GOOMBA_OFF: usize = 2;
pub const MICRO_GOOMBA_CELLS: u32 = 3;

/// Boomerang Bro (type 0x82): `KOOPA_CELL_W` x `KOOPA_CELL_H` cells with the object's
/// position at (`KOOPA_ORIGIN.0`, `KOOPA_ORIGIN.1 - BRO_RAISE`) (the body hangs 8 px below
/// it and reaches 32 px down); cells by `boomerang_bro_cell`.
pub fn boomerang_bro_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("boomerang_bro.png")
}
pub const BRO_RAISE: i32 = 16;
pub const BRO_CELLS: u32 = 8;
pub fn boomerang_bro_cell(right: bool, throwing: bool, step: usize) -> usize {
    right as usize * 4 + throwing as usize * 2 + step
}
/// The boomerang (projectile type 2): `KOOPA_CELL_W` x `KOOPA_CELL_H` cells, the
/// projectile's position at `KOOPA_ORIGIN`; spin frames flying right, then left.
pub fn boomerang_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("boomerang.png")
}
pub const BOOMERANG_FRAMES: u32 = 4;

/// Puff of smoke left by an enemy knocked out that has no body to fall (object state 8,
/// e.g. a Piranha Plant hit by the tail): four 16x16 cells in the order they are shown.
pub fn puff_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("puff.png")
}
pub const PUFF_FRAMES: u32 = 4;

/// Goal roulette card (object type 0x41): `KOOPA_CELL_W` x `KOOPA_CELL_H` cells, the object's
/// position at `KOOPA_ORIGIN`. Cells 0-2: the roulette (mushroom, flower, star); 3-5: the
/// same cards flying up after the touch; 6: the dark box around the roulette.
pub fn goal_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("goal.png")
}
pub const GOAL_FRAMES: u32 = 7;
pub const GOAL_FLYING: usize = 3;
pub const GOAL_BOX: usize = 6;

/// The course-clear line and the card line with its frame (text 0xAF / 0xB0, drawn into
/// the BG map at VRAM 0xA000; English: "LEVEL CLEARED!", "YOU GOT A PANEL"), rendered
/// from the ROM's font and BG palette 0.
pub fn course_clear_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("course_clear.png")
}
pub fn card_got_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("card_got.png")
}
/// Screen tile cells (column, row) of the two texts' top-left corners, of the card in its
/// frame, and their sizes in tiles.
pub const COURSE_CLEAR_AT: (u32, u32) = (9, 4);
pub const COURSE_CLEAR_TILES: (u32, u32) = (14, 2);
pub const CARD_GOT_AT: (u32, u32) = (6, 6);
pub const CARD_GOT_TILES: (u32, u32) = (20, 4);
pub const CLEAR_CARD_AT: (u32, u32) = (23, 7);

/// Player frames of one power-up form (0 small, 1 big, 2 fire, 3 raccoon): 32x32 cells, one per
/// pose slot below. The body fills the left 16 columns with the feet on the bottom row;
/// the raccoon's tail reaches into the right half. The art faces left.
pub fn player_path(dir: &Path, form: u8) -> PathBuf {
    dir.join("objects").join(format!("player_form{form}.png"))
}

/// The grow / shrink animation (0x080A8000 while player +0x3B runs down from 0x2F): 12
/// cells of `PLAYER_W` x `PLAYER_H`, pose 0x0813DE2F[i] drawn with the big form's
/// graphics. Each step shows for 4 frames.
pub fn player_grow_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("player_grow.png")
}
pub const PLAYER_GROW_STEPS: u32 = 12;

/// Player pose slots (cells of a player sheet).
pub const PLAYER_WALK: usize = 0; // 4 walk-cycle steps
pub const PLAYER_RUN: usize = 4; // 4 steps at P-speed
pub const PLAYER_JUMP: usize = 8;
pub const PLAYER_FALL: usize = 9;
pub const PLAYER_FLY_UP: usize = 10; // 3 tail-wag steps
pub const PLAYER_FLY_DOWN: usize = 13; // 3
pub const PLAYER_GLIDE: usize = 16; // 3
pub const PLAYER_WHIP: usize = 19; // 5 tail-whip steps on the ground
pub const PLAYER_WHIP_AIR: usize = 24; // 5 in the air
/// Death pose (0x4B, always drawn with the small form's graphics: 0x080A8FC0).
pub const PLAYER_DEATH: usize = 29;
/// Skidding: on the ground, holding against the motion (0x08097800: 0x0813DE41[form != 0]).
pub const PLAYER_SKID: usize = 30;
/// Fire Mario throwing a fireball: 3 steps on the ground, 3 in the air (0x0813DE9E).
pub const PLAYER_THROW: usize = 31;
pub const PLAYER_THROW_AIR: usize = 34;
/// Swimming (0x08097724): 6 steps of a stroke (0x0813DE43), then 4 of paddling in place
/// (0x0813DE4F); the small form has its own poses (+6 / +4 in those tables).
pub const PLAYER_SWIM: usize = 37;
pub const PLAYER_SWIM_IDLE: usize = 43;
pub const PLAYER_FRAMES: u32 = 47;
/// Star flashing: the player is drawn in palette 7 = 0x08133210[k], k = 0 being the
/// form's own palette (`player_path`), 1-3 in `player_star_path`.
pub const PLAYER_STAR_PALETTES: u32 = 4;
pub fn player_star_path(dir: &Path, form: u8, k: u32) -> PathBuf {
    dir.join("objects").join(format!("player_form{form}_star{k}.png"))
}
pub const PLAYER_W: u32 = 32;
pub const PLAYER_H: u32 = 32;
/// Width of the body part of a player cell.
pub const PLAYER_BODY_W: u32 = 16;

/// Water splash (effect slots 0x03007848 + 0x888, drawn by 0x0808A128): 16x16 cells, one per
/// step (timer >> 3): OBJ tile 0x081709BA[step] and the tile below it (+0x20) on the left,
/// the same mirrored on the right, palette 1.
pub fn splash_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("splash.png")
}
pub const SPLASH_FRAMES: u32 = 6;

/// Air bubble (slots 0x03007848 + 0x858, 0x08089E9C): one 8x8 OBJ tile 0x223, palette 1.
pub fn bubble_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("bubble.png")
}

/// Skid dust (0x0808C270): 8x8 cells, OBJ tiles 0x08170A88[i] (0x21D, 0x21E, 0x23D,
/// 0x23E), palette 0.
pub fn skid_dust_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("skid_dust.png")
}
pub const SKID_DUST_FRAMES: u32 = 4;

/// Fire Flower (object type 0x19) and Starman (0x0C): one 16x16 image each.
pub fn fire_flower_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("fire_flower.png")
}
pub fn star_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("star.png")
}

/// Super Leaf (object type 0x1E): one 16x16 image.
pub fn leaf_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("leaf.png")
}

impl Rgba {
    /// Splits a sheet of equal cells laid out left to right.
    pub fn cells(&self, w: u32, h: u32) -> Vec<Rgba> {
        (0..self.width / w)
            .map(|c| {
                let mut pixels = Vec::with_capacity((w * h * 4) as usize);
                for y in 0..h.min(self.height) {
                    let o = ((y * self.width + c * w) * 4) as usize;
                    pixels.extend_from_slice(&self.pixels[o..o + (w * 4) as usize]);
                }
                Rgba { width: w, height: h.min(self.height), pixels }
            })
            .collect()
    }
}

pub fn gfx_path(dir: &Path, rom_addr: u32) -> PathBuf {
    dir.join("gfx").join(format!("{rom_addr:08x}.png"))
}

pub fn write_manifest(dir: &Path, text: &str) -> Result<()> {
    write(&dir.join("manifest.txt"), text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_round_trip() {
        let dir = std::env::temp_dir().join(format!("sma4-assets-test-{}", std::process::id()));
        let lvl = LevelAsset {
            level_id: 0x22,
            tileset: 1,
            width: 2,
            height: 2,
            backdrop: [1, 2, 3],
            solidity: Solidity {
                top: [0x25, 0x50, 0xA0, 0xE2],
                solid: [0x2D, 0x53, 0xAD, 0xF0],
                high: std::array::from_fn(|i| i as u8 * 3),
            },
            metatiles: vec![0x80, 0x53, 0x150, 0x00],
            rules: BlockRules {
                bump_behaviour: [7; 256],
                final_metatile: [0x5F; 32],
                bounce: [-1, 2, -3, 4, -5, 6, -7, 8, -9, 10, -11],
                ..BlockRules::default()
            },
            enemies: vec![
                EnemySpawn { kind: 0x72, col: 14, row: 25, extra: [0; 2] },
                EnemySpawn { kind: 0xC8, col: 300, row: 3, extra: [0x85, 0x50] },
            ],
            time_limit: 300,
            start_y: 0x140,
            start_camera_y: 0xF0,
            pipe_flags: 0x80,
            water: [0xFF, 0xFF, 0xFF, 0xC1],
            camera: CameraParams { bottom: 0x1AF, fixed: 0, dead_zone: [0x10, 0x30], dead_zone_flying: [0x10, 0x30], top: 7 },
            junction: Some(Junction {
                level: 0x23,
                enemies: 2,
                tileset: 1,
                exits: vec![
                    PipeExit { page: 8, x: 0x88, y: 0xF0, dir: 2, camera_y: 0xF0 },
                    PipeExit { page: 9, x: 0x918, y: 0x180, dir: 1, camera_y: 0x110 },
                ],
            }),
            camera_x: Some((0x70, 0xA0)),
            slopes: Some(Slopes {
                shape: std::array::from_fn(|i| (i % SLOPE_SHAPES) as u8),
                height: std::array::from_fn(|i| [i as u8; 16]),
                no_floor: std::array::from_fn(|i| i % 3 == 0),
                push: std::array::from_fn(|i| i as i8 - 3),
            }),
            background: Some(Background {
                scene: 5,
                follow_y: true,
                image: Rgba { width: 2, height: 1, pixels: vec![1, 2, 3, 255, 0, 0, 0, 0] },
            }),
            atlas: Rgba {
                width: METATILE,
                height: METATILE * METATILES * ANIM_FRAMES,
                pixels: (0..(METATILE * METATILE * METATILES * ANIM_FRAMES * 4)).map(|i| i as u8).collect(),
            },
        };
        lvl.write(&dir).unwrap();
        assert_eq!(LevelAsset::read(&dir, 0x22).unwrap(), lvl);
        assert_eq!(lvl.kind(1, 0), TileKind::Solid);
        assert_eq!(lvl.kind(0, 1), TileKind::Empty);
        let plain = LevelAsset { junction: None, camera_x: None, slopes: None, background: None, ..lvl };
        plain.write(&dir).unwrap();
        assert_eq!(LevelAsset::read(&dir, 0x22).unwrap(), plain);
        std::fs::remove_dir_all(dir).ok();
    }
}

/// Toad House message box (BG3, fixed on screen): `TOAD_BOX_W` x `TOAD_BOX_H` tiles at
/// screen tile `TOAD_BOX_AT`.
pub fn toad_box_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("toad_box.png")
}
/// The box with the whole message of the 1-3 house drawn in.
pub fn toad_text_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("toad_text.png")
}
/// That message's bytes: BG tile ids, 0xF0 next line, 0xFE skip a cell, 0xFF end.
pub fn toad_message_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("toad_message.bin")
}
pub const TOAD_BOX_W: u32 = 0x12;
pub const TOAD_BOX_H: u32 = 8;
pub const TOAD_BOX_AT: (u32, u32) = (6, 1);
/// First text cell, in box tiles (map index 0x47: row 2, column 7).
pub const TOAD_TEXT_AT: (u32, u32) = (1, 1);

/// Toad (`KOOPA_CELL_W` x `KOOPA_CELL_H` cells like the other OAM renders, the object's
/// position at (`KOOPA_ORIGIN.0`, `KOOPA_ORIGIN.1 - TOAD_RAISE`)): facing left, facing
/// right.
pub fn toad_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("toad.png")
}
/// The item in World 1-3's hidden house's chest, 16x16: the Warp Whistle (item 0x0C).
pub fn house_item_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("warp_whistle.png")
}
pub const HOUSE_ITEM: u8 = 0x0C;
pub const TOAD_RAISE: i32 = 16;
/// The open chest's lid, 16x16 BG tiles drawn over the level at the chest.
pub fn chest_lid_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("chest_open.png")
}

/// One autoscroll script: per entry a command (bits 3-2 / 1-0: x / y speed step index, high
/// nibble: sub-script + 1) and a duration (steps of 4 frames, or the sub-script's repeat
/// count), and the sub-script + 1 to loop at the end (0: stop).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScrollScript {
    pub commands: Vec<u8>,
    pub durations: Vec<u8>,
    pub then: u8,
}

/// Autoscroll scripts (object 0x1A7: 0x0804B0A4 starts one, 0x080925B4 runs it), read
/// from the ROM: speed steps 0x08170E14, scripts 0x08170F4C / 0x081710E0 / 0x081711F1,
/// sub-scripts 0x08171170 / 0x081711C4.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutoscrollScripts {
    /// Speed change per step index (1/16 px per frame).
    pub steps: [i8; 4],
    pub scripts: Vec<ScrollScript>,
    /// Sub-scripts (`then` unused).
    pub subs: Vec<ScrollScript>,
}

pub fn autoscroll_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("autoscroll.bin")
}

impl AutoscrollScripts {
    pub fn write(&self, dir: &Path) -> Result<()> {
        let mut out: Vec<u8> = self.steps.iter().map(|&s| s as u8).collect();
        for list in [&self.scripts, &self.subs] {
            out.push(list.len() as u8);
            for s in list.iter() {
                out.push(s.commands.len() as u8);
                out.extend_from_slice(&s.commands);
                out.extend_from_slice(&s.durations);
                out.push(s.then);
            }
        }
        write(&autoscroll_path(dir), &out)
    }

    pub fn read(dir: &Path) -> Result<Self> {
        let path = autoscroll_path(dir);
        let d = read(&path)?;
        let parse = || -> Option<Self> {
            let mut p = 4;
            let mut lists = [Vec::new(), Vec::new()];
            for list in &mut lists {
                let n = *d.get(p)? as usize;
                p += 1;
                for _ in 0..n {
                    let len = *d.get(p)? as usize;
                    let commands = d.get(p + 1..p + 1 + len)?.to_vec();
                    let durations = d.get(p + 1 + len..p + 1 + 2 * len)?.to_vec();
                    let then = *d.get(p + 1 + 2 * len)?;
                    p += 2 + 2 * len;
                    list.push(ScrollScript { commands, durations, then });
                }
            }
            let [scripts, subs] = lists;
            Some(Self { steps: std::array::from_fn(|i| d[i] as i8), scripts, subs })
        };
        if d.len() < 4 {
            return Err(AssetError::BadLevel(path));
        }
        parse().ok_or(AssetError::BadLevel(path))
    }
}

#[cfg(test)]
mod autoscroll_tests {
    use super::*;

    #[test]
    fn autoscroll_scripts_round_trip() {
        let dir = std::env::temp_dir().join(format!("sma4-autoscroll-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("objects")).unwrap();
        let s = AutoscrollScripts {
            steps: [0, 1, -1, -1],
            scripts: vec![ScrollScript { commands: vec![4, 0x10, 0], durations: vec![8, 3, 0x86], then: 0 }],
            subs: vec![ScrollScript { commands: vec![0], durations: vec![0xFF], then: 0 }],
        };
        s.write(&dir).unwrap();
        assert_eq!(AutoscrollScripts::read(&dir).unwrap(), s);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

/// World 1-4's falling lift (type 0x36): one `LIFT_CELL` image, the object's position at
/// `LIFT_ORIGIN`.
pub fn lift_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("lift.png")
}
pub const LIFT_CELL: (u32, u32) = (64, 32);
pub const LIFT_ORIGIN: (i32, i32) = (8, 8);

/// Where each world's end-of-level pipes lead (transition 4, 0x080056A0): level
/// 0x08131EDA[world], enemy list 0x08131EEA[world], tileset 0x08131EFA[world]; the player
/// comes up out of a pipe there with its frame at `END_ROOM_ARRIVAL`, camera y 0xF0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EndRoom {
    pub level: u16,
    pub enemies: u16,
    pub tileset: u8,
}
pub const END_ROOM_ARRIVAL: (u16, u16) = (0x28, 0x180);
pub const END_ROOM_CAMERA_Y: u16 = 0xF0;
pub const WORLDS: usize = 8;

pub fn end_rooms_path(dir: &Path) -> PathBuf {
    dir.join("objects").join("end_rooms.bin")
}

pub fn write_end_rooms(dir: &Path, rooms: &[EndRoom; WORLDS]) -> Result<()> {
    let mut out = Vec::new();
    for r in rooms {
        out.extend_from_slice(&r.level.to_le_bytes());
        out.extend_from_slice(&r.enemies.to_le_bytes());
        out.push(r.tileset);
    }
    write(&end_rooms_path(dir), &out)
}

pub fn read_end_rooms(dir: &Path) -> Result<[EndRoom; WORLDS]> {
    let path = end_rooms_path(dir);
    let d = read(&path)?;
    if d.len() < WORLDS * 5 {
        return Err(AssetError::BadLevel(path));
    }
    Ok(std::array::from_fn(|i| {
        let e = &d[i * 5..i * 5 + 5];
        EndRoom { level: u16::from_le_bytes([e[0], e[1]]), enemies: u16::from_le_bytes([e[2], e[3]]), tileset: e[4] }
    }))
}

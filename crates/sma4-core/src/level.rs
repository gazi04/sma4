//! The live level grid: metatiles that can change during play (hit blocks), with
//! collision derived from the tileset's solidity thresholds.

use sma4_assets::{BlockRules, Junction, SLOPE_SHAPES, Slopes, Solidity, TileKind};

pub const TILE_PX: i32 = 16;

/// Metatile used for empty sky (the tileset setup pre-fills levels with it).
pub const SKY: u16 = 0x80;

#[derive(Debug, Clone, Default)]
pub struct Level {
    pub width: usize,
    pub height: usize,
    metatiles: Vec<u16>,
    pub solidity: Solidity,
    pub rules: BlockRules,
    /// Cells changed since the renderer last drained this list.
    pub changed: Vec<(usize, usize)>,
    /// Level id (0x03003B0A) of this area, so views know which assets to draw.
    pub id: u16,
    /// Where its pipes lead.
    pub junction: Option<Junction>,
    /// Camera limits the ROM sets for this area: x (min, max), and a fixed y.
    pub camera_x: Option<(i32, i32)>,
    pub camera_y: Option<i32>,
    /// Slope collision, in tilesets with slopes.
    pub slopes: Option<Slopes>,
    /// The multi-coin block in use (0x03003D00), its timer (0x03003CFE: 200 frames from the
    /// first hit) and its result high nibble.
    pub coin_block: Option<(i32, i32)>,
    pub coin_timer: u8,
    pub coin_hi: u8,
}

impl Level {
    pub fn new(width: usize, height: usize, metatiles: Vec<u16>, solidity: Solidity, rules: BlockRules) -> Self {
        assert_eq!(metatiles.len(), width * height);
        Self {
            width,
            height,
            metatiles,
            solidity,
            rules,
            changed: Vec::new(),
            id: 0,
            junction: None,
            camera_x: None,
            camera_y: None,
            slopes: None,
            coin_block: None,
            coin_timer: 0,
            coin_hi: 0,
        }
    }

    pub fn with_slopes(mut self, slopes: Option<Slopes>) -> Self {
        self.slopes = slopes;
        self
    }

    /// The area's id, pipe destination and camera limits.
    pub fn with_area(mut self, id: u16, junction: Option<Junction>, camera_x: Option<(i32, i32)>) -> Self {
        self.id = id;
        self.junction = junction;
        self.camera_x = camera_x;
        self
    }

    fn index(&self, tx: i32, ty: i32) -> Option<usize> {
        (tx >= 0 && ty >= 0 && (tx as usize) < self.width && (ty as usize) < self.height)
            .then(|| ty as usize * self.width + tx as usize)
    }

    pub fn metatile(&self, tx: i32, ty: i32) -> Option<u16> {
        self.index(tx, ty).map(|i| self.metatiles[i])
    }

    pub fn set(&mut self, tx: i32, ty: i32, m: u16) {
        if let Some(i) = self.index(tx, ty) {
            self.metatiles[i] = m;
            self.changed.push((tx as usize, ty as usize));
        }
    }

    /// Outside the map: walls on the left/right edges, open above and below.
    pub fn at(&self, tx: i32, ty: i32) -> TileKind {
        if tx < 0 || tx >= self.width as i32 {
            return TileKind::Solid;
        }
        match self.metatile(tx, ty) {
            Some(m) => self.solidity.kind(m),
            None => TileKind::Empty,
        }
    }

    /// Slope shape of a cell (0 = none, or no slopes in this tileset).
    pub fn slope(&self, tx: i32, ty: i32) -> u8 {
        match (&self.slopes, self.metatile(tx, ty)) {
            (Some(s), Some(m)) => s.shape_of(&self.solidity, m).min(SLOPE_SHAPES as u8 - 1),
            _ => 0,
        }
    }

    /// The floor of a slope shape at pixel column `x` of its cell: y offset 0-15 from the
    /// top of the cell, or `None` for shapes without a floor (0x0809A7E8).
    pub fn slope_floor(&self, shape: u8, x: i32) -> Option<i32> {
        let s = self.slopes.as_ref()?;
        let k = shape as usize;
        (!s.no_floor[k]).then(|| (s.height[k][x.rem_euclid(TILE_PX) as usize] & 0xF) as i32)
    }

    /// The id a cell collides as (second-plane ids mapped, `Solidity::id`).
    pub fn collision_id(&self, tx: i32, ty: i32) -> Option<u8> {
        self.metatile(tx, ty).and_then(|m| self.solidity.id(m))
    }

    /// Whether this tileset has slope collision (0x03003CF4).
    pub fn has_slopes(&self) -> bool {
        self.slopes.is_some()
    }

    /// The low nibble of a shape's height map (0x0816A029) at pixel column `x`, whatever
    /// the shape: the enemies' floor test (0x08045ABC) reads it for any shape.
    pub fn slope_height(&self, shape: u8, x: i32) -> i32 {
        self.slopes.as_ref().map_or(0, |s| (s.height[shape as usize][x.rem_euclid(TILE_PX) as usize] & 0xF) as i32)
    }

    /// Push down the slope of a shape (signed speed units), 0 on flat ground.
    pub fn slope_push(&self, shape: u8) -> i32 {
        self.slopes.as_ref().map_or(0, |s| s.push[shape as usize] as i32)
    }

    pub fn pixel_width(&self) -> i32 {
        self.width as i32 * TILE_PX
    }

    pub fn pixel_height(&self) -> i32 {
        self.height as i32 * TILE_PX
    }

    /// Test helper: a level built from collision kinds, using SMB3-like thresholds.
    pub fn from_kinds(width: usize, height: usize, kinds: &[TileKind]) -> Self {
        let metatiles = kinds
            .iter()
            .map(|k| match k {
                TileKind::Empty => SKY,
                TileKind::Top => 0x50,
                TileKind::Solid => 0x53,
            })
            .collect();
        Self::new(width, height, metatiles, TEST_SOLIDITY, BlockRules::default())
    }
}

/// Thresholds of tileset 1, used by tests.
pub const TEST_SOLIDITY: Solidity =
    Solidity { top: [0x25, 0x50, 0xA0, 0xE2], solid: [0x2D, 0x53, 0xAD, 0xF0], high: [0; sma4_assets::HIGH_IDS] };

/// 24x8 hills level (tileset 3 thresholds): flat ground on row 7, a 45-degree hill up
/// from column 6 to a flat top on row 4 (columns 9-12) and back down to column 15.
#[cfg(test)]
pub fn test_hill() -> Level {
    use sma4_assets::{BlockRules, Slopes, Solidity};
    const UP: u16 = 0x99;
    const FLAT: u16 = 0x9A;
    const DOWN: u16 = 0x9B;
    const FILL: u16 = 0xAB;
    const SKY: u16 = 0x80;
    let (w, h) = (24, 8);
    let mut m = vec![SKY; w * h];
    let mut put = |x: usize, y: usize, t: u16| m[y * w + x] = t;
    for x in 0..w {
        put(x, 7, FLAT);
    }
    for (x, top, t) in [(6, 6, UP), (7, 5, UP), (8, 4, UP), (13, 4, DOWN), (14, 5, DOWN), (15, 6, DOWN)] {
        put(x, top, t);
        (top + 1..h).for_each(|y| put(x, y, FILL));
    }
    for x in 9..13 {
        put(x, 4, FLAT);
        (5..h).for_each(|y| put(x, y, FILL));
    }
    // Shapes 1 (up), 2 (down), 4 (fill), 7 (flat) as in the ROM tables.
    let mut slopes = Slopes::default();
    for (id, shape) in [(UP, 1), (FLAT, 7), (DOWN, 2), (FILL, 4)] {
        slopes.shape[id as usize] = shape;
    }
    slopes.height[1] = std::array::from_fn(|x| 15 - x as u8);
    slopes.height[2] = std::array::from_fn(|x| x as u8);
    for k in [1, 2, 4, 7] {
        slopes.no_floor[k] = false;
    }
    slopes.height[4] = [0; 16];
    slopes.height[7] = [0; 16];
    slopes.push[1] = -3;
    slopes.push[2] = 3;
    let solidity =
        Solidity { top: [0x25, 0x5F, 0x99, 0xE2], solid: [0x2E, 0x5F, 0xA6, 0xF0], high: [0; sma4_assets::HIGH_IDS] };
    Level::new(w, h, m, solidity, BlockRules::default()).with_slopes(Some(slopes))
}

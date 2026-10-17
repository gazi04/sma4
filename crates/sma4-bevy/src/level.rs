//! Draws a level as one native Bevy tilemap from the extracted assets. The atlas holds
//! all 512 metatiles of the tileset as an array texture, so a cell's tile index is simply
//! its metatile value and hit blocks can change into any other metatile.
//!
//! World space: 1 unit = 1 GBA pixel, x right, y up, level top-left at the origin, so a
//! level pixel (px, py) sits at world (px, -py).

use std::collections::HashMap;

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    sprite_render::{TileData, TilemapChunk, TilemapChunkTileData},
};
use sma4_assets::{ANIM_FRAMES, LevelAsset, METATILE, METATILES, TileKind, anim_frame};
use sma4_core::{ItemPhase, Level, level::SKY, player::SUB};

use crate::GameRes;

/// Cell value while a block bounces; the bump sprite draws the block instead.
const BUMP_PLACEHOLDER: u16 = 0xF3;
/// Item sprite depth: behind the tilemap while it rises out of a block, in front after.
const Z_ITEM_EMERGING: f32 = -1.0;
const Z_ITEM: f32 = 5.0;
const Z_BUMP: f32 = 4.0;

#[derive(Resource)]
pub struct LevelRes(pub LevelAsset);

/// Debug: start column for the player (`--spawn-col`), instead of the default.
#[derive(Resource, Default)]
pub struct SpawnColumn(pub Option<i32>);

/// Debug: a score popup over the player at the start (`--popup`).
#[derive(Resource, Default)]
pub struct DebugPopup(pub Option<u8>);

/// Shows the `--popup` once the renderer is warmed up (frame 120).
fn debug_popup(mut game: ResMut<GameRes>, mut popup: ResMut<DebugPopup>) {
    if game.0.frame >= 120
        && let Some(kind) = popup.0.take()
    {
        let (x, y) = game.0.player.px();
        game.0.status.popup(x, y - 24, kind);
    }
}

/// How a run starts: on the map or in the level, and the debug overrides (`--hit`,
/// `--form`, `--lives`).
#[derive(Resource, Default)]
pub struct StartOptions {
    pub on_map: bool,
    /// Block cell to hit from below at the start.
    pub hit: Option<(i32, i32)>,
    /// Starting form: 0 small, 1 big, 3 raccoon.
    pub form: u8,
    pub lives: Option<u16>,
}

pub struct LevelPlugin;

impl Plugin for LevelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (spawn_level, load_item_art, load_coin_frames, load_sparkle_frames))
            .add_systems(Update, (debug_popup, load_pending_level))
            .add_systems(Update, (sync_tiles, animate_tiles, sync_bumps, sync_items, sync_coins, sync_sparkles));
    }
}

/// One tilemap per background animation frame (spinning coins and blocks); only the
/// current frame's is visible. Swapping visibility avoids rebuilding a chunk.
#[derive(Component)]
struct LevelTiles(usize);

#[derive(Component)]
struct BumpSprite;

#[derive(Component)]
struct ItemSprite;

#[derive(Component)]
struct CoinSprite;

/// Coin metatile of tileset 1: fallback when the extracted spin frames are missing.
const COIN_METATILE: u16 = 0x40;

/// Spin frames of the popping coin (extracted from the ROM's sprite graphics).
#[derive(Resource, Default)]
struct CoinFrames(Vec<Handle<Image>>);
const Z_COIN: f32 = 6.0;

/// Single-metatile images cut from the atlas on demand (for bouncing blocks).
#[derive(Resource, Default)]
struct MetatileImages(HashMap<u16, Handle<Image>>);

/// Item images: the mushroom and the Super Leaf.
#[derive(Resource)]
struct ItemArt {
    mushroom: Option<Handle<Image>>,
    leaf: Option<Handle<Image>>,
}

/// Coin-collect sparkle frames, 8x8, smallest first.
#[derive(Resource, Default)]
struct SparkleFrames(Vec<Handle<Image>>);

#[derive(Component)]
struct SparkleSprite;
/// Above the player (z 10), so the sparkle is never hidden behind it.
const Z_SPARKLE: f32 = 11.0;

fn tile(m: u16) -> Option<TileData> {
    (m != SKY && m != BUMP_PLACEHOLDER).then(|| TileData::from_tileset_index(m))
}

/// The level's tilemaps (one per animation frame), backdrop colour and metatile cache.
fn spawn_tilemaps(commands: &mut Commands, images: &mut Assets<Image>, level: &LevelAsset) {
    let frame_bytes = (METATILE * METATILE * METATILES * 4) as usize;
    let tilesets: Vec<Handle<Image>> = level
        .atlas
        .pixels
        .chunks_exact(frame_bytes)
        .take(ANIM_FRAMES as usize)
        .map(|pixels| {
            let mut atlas = Image::new(
                Extent3d { width: METATILE, height: METATILE * METATILES, depth_or_array_layers: 1 },
                TextureDimension::D2,
                pixels.to_vec(),
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            );
            atlas.reinterpret_stacked_2d_as_array(METATILES).expect("metatile atlas");
            images.add(atlas)
        })
        .collect();
    info!("level {:#x}: {}x{} metatiles", level.level_id, level.width, level.height);

    let size = UVec2::new(level.width as u32, level.height as u32);
    let tiles = level.metatiles.iter().map(|&m| tile(m)).collect();
    let half = Vec2::new(level.width as f32, level.height as f32) * METATILE as f32 / 2.0;
    let tiles: Vec<Option<TileData>> = tiles;
    for (i, tileset) in tilesets.into_iter().enumerate() {
        commands.spawn((
            LevelTiles(i),
            TilemapChunk { chunk_size: size, tile_display_size: UVec2::splat(METATILE), tileset, ..default() },
            TilemapChunkTileData::from_y_down_tiles(size, tiles.clone()),
            Transform::from_xyz(half.x, -half.y, 0.0),
            if i == 0 { Visibility::Inherited } else { Visibility::Hidden },
        ));
    }

    let [r, g, b] = level.backdrop;
    commands.insert_resource(ClearColor(Color::srgb_u8(r, g, b)));
    commands.insert_resource(MetatileImages::default());
}

fn core_level(level: &LevelAsset) -> Level {
    Level::new(level.width, level.height, level.metatiles.clone(), level.solidity, level.rules.clone())
        .with_slopes(level.slopes.clone())
}

fn spawn_level(
    mut commands: Commands,
    level: Res<LevelRes>,
    spawn_col: Option<Res<SpawnColumn>>,
    start: Option<Res<StartOptions>>,
    mut game: ResMut<GameRes>,
    mut images: ResMut<Assets<Image>>,
) {
    let level = &level.0;
    spawn_tilemaps(&mut commands, &mut images, level);
    let core_level = core_level(level);
    let spawn = match spawn_col.and_then(|c| c.0) {
        Some(col) => spawn_point(&core_level, col),
        None => start_point(level),
    };
    game.0.enter_level(core_level, spawn, &level.enemies);
    game.0.set_time_limit(level.time_limit);
    let start = start.map(|s| (s.on_map, s.hit, s.form, s.lives)).unwrap_or_default();
    let (on_map, hit, form, lives) = start;
    if let Some(n) = lives {
        game.0.status.lives = n.saturating_sub(1);
    }
    if on_map {
        game.0.screen = sma4_core::Screen::Map;
    }
    match form {
        1 => game.0.player.set_form(sma4_core::Form::Big),
        3 => game.0.player.set_form(sma4_core::Form::Raccoon),
        _ => {}
    }
    if let Some((tx, ty)) = hit {
        let g = &mut game.0;
        if let Some((bump, _)) = sma4_core::blocks::hit_block(&mut g.level, &g.player, tx, ty) {
            g.bumps.push(bump);
        }
    }
}

/// A spot entered on the world map: load its level (extracted by `sma4-extract`) and play
/// it. Spots without a level asset (Toad houses, the spade game) just return to the map.
fn load_pending_level(
    mut commands: Commands,
    mut game: ResMut<GameRes>,
    dir: Res<crate::AssetDir>,
    mut level_res: ResMut<LevelRes>,
    mut images: ResMut<Assets<Image>>,
    old: Query<Entity, With<LevelTiles>>,
) {
    let Some(node) = game.0.pending_level.take() else { return };
    let level = match LevelAsset::read(&dir.0, node.level) {
        Ok(l) if node.level & 0x8000 == 0 => l,
        result => {
            if let Err(e) = result {
                warn!("{e}");
            }
            info!("map spot {:#x} (tileset {}) has no playable level here", node.level, node.tileset);
            if let Some(map) = &mut game.0.world_map {
                map.show(false);
            }
            return;
        }
    };
    for e in &old {
        commands.entity(e).despawn();
    }
    spawn_tilemaps(&mut commands, &mut images, &level);
    let core = core_level(&level);
    let spawn = start_point(&level);
    info!("level {:#x}: {}x{} metatiles", level.level_id, level.width, level.height);
    game.0.play(core, spawn, &level.enemies, level.time_limit);
    level_res.0 = level;
}

/// Pushes cells the game changed (hit blocks, collected coins) into the tilemaps; redraws
/// everything when the level was restored (a new attempt from the map).
fn sync_tiles(
    mut game: ResMut<GameRes>,
    mut generation: Local<u32>,
    mut maps: Query<&mut TilemapChunkTileData, With<LevelTiles>>,
) {
    let restored = game.0.level_generation != *generation;
    *generation = game.0.level_generation;
    let level = &mut game.0.level;
    let (w, h) = (level.width, level.height);
    if restored {
        level.changed.clear();
        for mut tiles in &mut maps {
            for ty in 0..h {
                for tx in 0..w {
                    tiles.0[(h - 1 - ty) * w + tx] = tile(level.metatile(tx as i32, ty as i32).unwrap_or(SKY));
                }
            }
        }
        return;
    }
    if level.changed.is_empty() {
        return;
    }
    for (tx, ty) in std::mem::take(&mut level.changed) {
        let m = level.metatile(tx as i32, ty as i32).unwrap_or(SKY);
        for mut tiles in &mut maps {
            // Tile data is stored bottom row first.
            tiles.0[(h - 1 - ty) * w + tx] = tile(m);
        }
    }
}

/// Background tile animation: shows the tilemap of the current animation frame.
fn animate_tiles(game: Res<GameRes>, mut maps: Query<(&LevelTiles, &mut Visibility)>) {
    let current = anim_frame(game.0.frame);
    for (map, mut vis) in &mut maps {
        let want = if map.0 == current { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
    }
}

fn metatile_image(cache: &mut MetatileImages, level: &LevelAsset, images: &mut Assets<Image>, m: u16) -> Handle<Image> {
    cache
        .0
        .entry(m)
        .or_insert_with(|| {
            let bytes = (METATILE * METATILE * 4) as usize;
            let start = m as usize * bytes;
            images.add(Image::new(
                Extent3d { width: METATILE, height: METATILE, depth_or_array_layers: 1 },
                TextureDimension::D2,
                level.atlas.pixels[start..start + bytes].to_vec(),
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
            ))
        })
        .clone()
}

/// Bouncing blocks: drawn with the tile they turn into, lifted by the bounce offset.
fn sync_bumps(
    mut commands: Commands,
    game: Res<GameRes>,
    level: Res<LevelRes>,
    mut cache: ResMut<MetatileImages>,
    mut images: ResMut<Assets<Image>>,
    old: Query<Entity, With<BumpSprite>>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    for b in &game.0.bumps {
        let image = metatile_image(&mut cache, &level.0, &mut images, b.final_metatile);
        let x = (b.tx * 16 + 8) as f32;
        let y = -((b.ty * 16 + 8) as f32) + (b.lift / SUB) as f32;
        commands.spawn((BumpSprite, Sprite::from_image(image), Transform::from_xyz(x, y, Z_BUMP)));
    }
}

fn sync_items(mut commands: Commands, game: Res<GameRes>, art: Res<ItemArt>, old: Query<Entity, With<ItemSprite>>) {
    for e in &old {
        commands.entity(e).despawn();
    }
    for item in &game.0.items {
        let leaf = item.kind == sma4_core::blocks::LEAF;
        let Some(image) = (if leaf { &art.leaf } else { &art.mushroom }) else { continue };
        let (x, y) = item.px();
        let z = if matches!(item.phase, ItemPhase::Emerging { .. }) { Z_ITEM_EMERGING } else { Z_ITEM };
        let mut sprite = Sprite::from_image(image.clone());
        // 0x0804EBAC: the leaf faces the way it swings.
        sprite.flip_x = leaf && item.vx > 0;
        commands.spawn((
            ItemSprite,
            sprite,
            Transform::from_xyz((x + 8) as f32, -((y + 8) as f32), z),
        ));
    }
}

/// Cuts `objects/coin_spin.png` into one image per frame.
fn load_coin_frames(mut commands: Commands, dir: Res<crate::AssetDir>, mut images: ResMut<Assets<Image>>) {
    let path = sma4_assets::coin_spin_path(&dir.0);
    let frames = match sma4_assets::Rgba::read_png(&path) {
        Ok(sheet) => (0..sheet.width / 16)
            .map(|f| {
                let mut px = Vec::with_capacity(16 * 16 * 4);
                for y in 0..16 {
                    let o = ((y * sheet.width + f * 16) * 4) as usize;
                    px.extend_from_slice(&sheet.pixels[o..o + 16 * 4]);
                }
                images.add(Image::new(
                    Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
                    TextureDimension::D2,
                    px,
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
                ))
            })
            .collect(),
        Err(e) => {
            warn!("{e}; coins will not spin (re-run sma4-extract)");
            Vec::new()
        }
    };
    commands.insert_resource(CoinFrames(frames));
}

fn sync_coins(
    mut commands: Commands,
    game: Res<GameRes>,
    level: Res<LevelRes>,
    frames: Res<CoinFrames>,
    mut cache: ResMut<MetatileImages>,
    mut images: ResMut<Assets<Image>>,
    old: Query<Entity, With<CoinSprite>>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    for c in &game.0.coin_pops {
        let image = match frames.0.get(c.frame()) {
            Some(f) => f.clone(),
            None => metatile_image(&mut cache, &level.0, &mut images, COIN_METATILE),
        };
        commands.spawn((
            CoinSprite,
            Sprite::from_image(image),
            Transform::from_xyz((c.x + 8) as f32, -((c.y + 8) as f32), Z_COIN),
        ));
    }
}

fn load_sparkle_frames(mut commands: Commands, dir: Res<crate::AssetDir>, mut images: ResMut<Assets<Image>>) {
    let frames = crate::player::load_cells(&sma4_assets::sparkle_path(&dir.0), 8, 8, &mut images);
    commands.insert_resource(SparkleFrames(frames));
}

/// Sparkles where coins were collected: four 8x8 pieces each (top-left anchored).
fn sync_sparkles(
    mut commands: Commands,
    game: Res<GameRes>,
    frames: Res<SparkleFrames>,
    old: Query<Entity, With<SparkleSprite>>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    for s in &game.0.sparkles {
        for (x, y, f) in s.pieces() {
            let Some(image) = frames.0.get(f) else { continue };
            commands.spawn((
                SparkleSprite,
                Sprite::from_image(image.clone()),
                Transform::from_xyz((x + 4) as f32, -((y + 4) as f32), Z_SPARKLE),
            ));
        }
    }
}

/// The growth item (mushroom), extracted from the ROM (`objects/growth_item.png`).
fn load_item_art(mut commands: Commands, dir: Res<crate::AssetDir>, mut images: ResMut<Assets<Image>>) {
    let mut load = |path: std::path::PathBuf| crate::player::load_cells(&path, 16, 16, &mut images).into_iter().next();
    let mushroom = load(sma4_assets::growth_item_path(&dir.0));
    let leaf = load(sma4_assets::leaf_path(&dir.0));
    commands.insert_resource(ItemArt { mushroom, leaf });
}

/// TODO(re): use the start position from the level header (byte 4, bits 7-5).
/// For now: column `tx`, standing on the lowest surface (normally the ground).
/// Where a level starts (0x08014F20): the header's start y (top of the 32 px frame, so a
/// small player's box is 16 px lower); x near the left edge, like `--spawn-col 2`.
// TODO(re): the start x is set outside the level loader; SMB3 starts at x 0x18.
fn start_point(level: &LevelAsset) -> (i32, i32) {
    (2 * sma4_core::level::TILE_PX + 2, level.start_y as i32 + 32 - 16)
}

fn spawn_point(map: &Level, tx: i32) -> (i32, i32) {
    use sma4_core::level::TILE_PX;
    let ty = (1..map.height as i32)
        .rev()
        .find(|&ty| map.at(tx, ty) != TileKind::Empty && map.at(tx, ty - 1) == TileKind::Empty)
        .unwrap_or(0);
    (tx * TILE_PX + 2, ty * TILE_PX - 16)
}

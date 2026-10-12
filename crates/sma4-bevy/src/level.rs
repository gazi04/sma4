//! Draws a level as one native Bevy tilemap from the extracted assets: the metatile
//! atlas becomes an array texture (one layer per metatile), the layout becomes tile data.
//!
//! World space: 1 unit = 1 GBA pixel, x right, y up, level top-left at the origin, so a
//! level pixel (px, py) sits at world (px, -py).

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    sprite_render::{TileData, TilemapChunk, TilemapChunkTileData},
};
use sma4_assets::{LevelAsset, METATILE, TileKind};
use sma4_core::Collision;

use crate::GameRes;

#[derive(Resource)]
pub struct LevelRes(pub LevelAsset);

pub struct LevelPlugin;

impl Plugin for LevelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_level);
    }
}

fn spawn_level(mut commands: Commands, level: Res<LevelRes>, mut game: ResMut<GameRes>, mut images: ResMut<Assets<Image>>) {
    let level = &level.0;
    let layers = level.atlas_layers();
    let mut atlas = Image::new(
        Extent3d { width: METATILE, height: level.atlas.height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        level.atlas.pixels.clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    atlas.reinterpret_stacked_2d_as_array(layers).expect("metatile atlas");
    info!("level {:#x}: {}x{} metatiles, {} atlas layers", level.level_id, level.width, level.height, layers);

    let size = UVec2::new(level.width as u32, level.height as u32);
    let tiles = level.layers.iter().map(|l| l.map(TileData::from_tileset_index)).collect();
    let half = Vec2::new(level.width as f32, level.height as f32) * METATILE as f32 / 2.0;
    commands.spawn((
        TilemapChunk {
            chunk_size: size,
            tile_display_size: UVec2::splat(METATILE),
            tileset: images.add(atlas),
            ..default()
        },
        TilemapChunkTileData::from_y_down_tiles(size, tiles),
        Transform::from_xyz(half.x, -half.y, 0.0),
    ));

    let [r, g, b] = level.backdrop;
    commands.insert_resource(ClearColor(Color::srgb_u8(r, g, b)));

    let collision = Collision { width: level.width, height: level.height, kinds: level.kinds.clone() };
    let spawn = spawn_point(&collision);
    game.0.enter_level(collision, spawn);
}

/// TODO(re): use the start position from the level header (byte 4, bits 7-5).
/// For now: column 2, standing on the first non-empty tile from the top.
fn spawn_point(map: &Collision) -> (i32, i32) {
    use sma4_core::player::{HITBOX_H, TILE_PX};
    let tx = 2;
    let ty = (0..map.height as i32).find(|&ty| map.at(tx, ty) != TileKind::Empty).unwrap_or(0);
    (tx * TILE_PX + 2, ty * TILE_PX - HITBOX_H)
}

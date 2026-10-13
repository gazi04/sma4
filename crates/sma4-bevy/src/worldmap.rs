//! The walkable world map, from `map/world1_walk.bin`, `_meta.png` (map metatiles, one
//! column per animation frame) and `_mario.png`, all extracted from the user's own ROM.
//! Walking, entering and clearing spots run in `sma4_core::world_map`; this draws the
//! tiles, Mario and the iris that closes on him before a level.

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use sma4_assets::{MAP_ANIM_FRAMES, MAP_MARIO_H, MAP_MARIO_W, WorldMapAsset};
use sma4_core::{Form, MapPhase, SCREEN_H, SCREEN_W, Screen, WorldMap};

use crate::GameRes;
use crate::player::sprite_image;

pub struct WorldMapPlugin;

impl Plugin for WorldMapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, spawn_map).add_systems(Update, sync_map);
    }
}

/// Over the static map view (z 41), under the panel (42) and the status bar (43).
const Z_TILES: f32 = 41.2;
const Z_MARIO: f32 = 41.5;
const Z_IRIS: f32 = 41.8;

/// Debug: map cells to mark cleared at the start (`--cleared`).
#[derive(Resource, Default)]
pub struct ClearedSpots(pub Vec<(usize, usize)>);

#[derive(Resource)]
struct MapArt {
    /// [frame][tile id]
    tiles: Vec<Vec<Handle<Image>>>,
    /// [form cell * 2 + phase]
    mario: Vec<Handle<Image>>,
    origin: (i32, i32),
    iris: Handle<Image>,
}

#[derive(Component)]
struct MapTile(usize);

#[derive(Component)]
struct MapMario;

#[derive(Component)]
struct Iris;

/// Camera-local transform of something `w` x `h` whose top-left is at GBA screen (x, y).
fn at(x: i32, y: i32, w: i32, h: i32, z: f32) -> Transform {
    Transform::from_xyz(
        x as f32 + w as f32 / 2.0 - SCREEN_W as f32 / 2.0,
        SCREEN_H as f32 / 2.0 - y as f32 - h as f32 / 2.0,
        z,
    )
}

fn spawn_map(
    mut commands: Commands,
    dir: Res<crate::AssetDir>,
    mut game: ResMut<GameRes>,
    mut images: ResMut<Assets<Image>>,
    camera: Single<Entity, With<Camera2d>>,
    cleared: Option<Res<ClearedSpots>>,
) {
    let asset = match WorldMapAsset::read(&dir.0, 0) {
        Ok(a) => a,
        Err(e) => {
            warn!("{e}; re-run sma4-extract for the walkable world map");
            return;
        }
    };
    // Atlas: 256 metatiles down, one 16-px column per animation frame.
    let cell = |f: u32, id: u32| {
        let a = &asset.atlas;
        let mut pixels = Vec::with_capacity(16 * 16 * 4);
        for y in id * 16..id * 16 + 16 {
            let o = ((y * a.width + f * 16) * 4) as usize;
            pixels.extend_from_slice(&a.pixels[o..o + 64]);
        }
        sma4_assets::Rgba { width: 16, height: 16, pixels }
    };
    let tiles = (0..MAP_ANIM_FRAMES).map(|f| (0..256).map(|id| images.add(sprite_image(cell(f, id)))).collect()).collect();
    let mario = asset.mario.cells(MAP_MARIO_W, MAP_MARIO_H).into_iter().map(|c| images.add(sprite_image(c))).collect();
    let iris = images.add(Image::new(
        Extent3d { width: SCREEN_W, height: SCREEN_H, depth_or_array_layers: 1 },
        TextureDimension::D2,
        vec![0; (SCREEN_W * SCREEN_H * 4) as usize],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    let origin = asset.origin;
    commands.entity(*camera).with_children(|cam| {
        // Only what the GBA screen shows: cells are cut at its edges.
        for r in 0..asset.rows {
            for c in 0..asset.cols {
                let (x, y) = (origin.0 + c as i32 * 16, origin.1 + r as i32 * 16);
                let (x0, x1) = (x.max(0), (x + 16).min(SCREEN_W as i32));
                let (y0, y1) = (y.max(0), (y + 16).min(SCREEN_H as i32));
                if x0 >= x1 || y0 >= y1 {
                    continue;
                }
                let rect = Rect::new((x0 - x) as f32, (y0 - y) as f32, (x1 - x) as f32, (y1 - y) as f32);
                let sprite = Sprite { rect: Some(rect), ..default() };
                let t = at(x0, y0, x1 - x0, y1 - y0, Z_TILES);
                cam.spawn((MapTile(r * asset.cols + c), sprite, t, Visibility::Hidden));
            }
        }
        cam.spawn((MapMario, Sprite::default(), Transform::default(), Visibility::Hidden));
        cam.spawn((Iris, Sprite::from_image(iris.clone()), at(0, 0, 240, 160, Z_IRIS), Visibility::Hidden));
    });
    let mut map =
        WorldMap::new(asset.grid, (asset.cols, asset.rows), asset.start, asset.thresholds, asset.nodes, asset.anim_ticks);
    for &(c, r) in cleared.iter().flat_map(|c| c.0.iter()) {
        map.clear_cell(c, r);
    }
    game.0.world_map = Some(map);
    commands.insert_resource(MapArt { tiles, mario, origin, iris });
}

/// Iris (0x0802F44C): everything outside a circle of `r` px round (cx, cy) is black.
fn draw_iris(image: &mut Image, (cx, cy): (i32, i32), r: i32) {
    let Some(data) = image.data.as_mut() else { return };
    for y in 0..SCREEN_H as i32 {
        for x in 0..SCREEN_W as i32 {
            let (dx, dy) = (x - cx, y - cy);
            let a = if dx * dx + dy * dy > r * r { 255 } else { 0 };
            data[((y * SCREEN_W as i32 + x) * 4 + 3) as usize] = a;
        }
    }
}

fn sync_map(
    game: Res<GameRes>,
    art: Option<Res<MapArt>>,
    mut images: ResMut<Assets<Image>>,
    mut tiles: Query<(&MapTile, &mut Sprite, &mut Visibility), (Without<MapMario>, Without<Iris>)>,
    mut mario: Query<(&mut Sprite, &mut Transform, &mut Visibility), (With<MapMario>, Without<Iris>)>,
    mut iris: Query<&mut Visibility, (With<Iris>, Without<MapTile>, Without<MapMario>)>,
) {
    let (Some(art), Some(map)) = (art, &game.0.world_map) else { return };
    let shown = game.0.screen == Screen::Map;
    let vis = |on: bool| if on { Visibility::Inherited } else { Visibility::Hidden };
    let frame = map.anim_frame as usize;
    for (tile, mut sprite, mut v) in &mut tiles {
        *v = vis(shown);
        if shown {
            let (c, r) = ((tile.0 % map.cols) as i32, (tile.0 / map.cols) as i32);
            let image = &art.tiles[frame][map.tile(c, r) as usize];
            if sprite.image != *image {
                sprite.image = image.clone();
            }
        }
    }
    // Mario: lower OBJ at (x, y - 25), the upper one 16 px above (0x0800C45C); the two
    // poses alternate on frame-counter bit 3.
    let (sx, sy) = (map.x as i32 + art.origin.0, map.y as i32 - 0x10 + art.origin.1);
    for (mut sprite, mut t, mut v) in &mut mario {
        *v = vis(shown);
        let form = match game.0.player.form {
            Form::Small => 0,
            Form::Big => 1,
            Form::Raccoon => 2,
        };
        let phase = (game.0.frame >> 3 & 1) as usize;
        if let Some(image) = art.mario.get(form * 2 + phase) {
            sprite.image = image.clone();
        }
        *t = at(sx, sy - 16, MAP_MARIO_W as i32, MAP_MARIO_H as i32, Z_MARIO);
    }
    let radius = match map.phase {
        MapPhase::Entering(r) if shown => Some(r as i32),
        _ => None,
    };
    for mut v in &mut iris {
        *v = vis(radius.is_some());
    }
    if let Some(r) = radius
        && let Some(mut image) = images.get_mut(&art.iris)
    {
        draw_iris(&mut image, (sx + 8, sy + 8), r);
    }
}

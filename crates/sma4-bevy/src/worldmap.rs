//! The walkable world map, from `map/world1_walk.bin`, `_meta.png` (map metatiles, one
//! column per animation frame) and `_mario.png`, all extracted from the user's own ROM.
//! Walking, entering and clearing spots run in `sma4_core::world_map`; this draws the
//! tiles, Mario and the iris that closes on him before a level. The game-over menu sits on
//! this map too: dimmed while its panel is up, then with Mario spinning as he flies
//! back to the checkpoint (`_mario_spin.png`). A world start brings Mario in with the
//! circle of stars (`_stars.png`, `sma4_core::map_stars`).

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use sma4_assets::{MAP_ANIM_FRAMES, MAP_MARIO_H, MAP_MARIO_W, MAP_SPIN_SIZE, MAP_STAR_SIZE, WorldMapAsset};
use sma4_core::screens::MenuMario;
use sma4_core::{Form, MapPhase, SCREEN_H, SCREEN_W, Screen, WorldMap};

use crate::GameRes;
use crate::player::sprite_image;

pub struct WorldMapPlugin;

impl Plugin for WorldMapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, spawn_map).add_systems(Update, (reload_world, sync_map, sync_warp).chain());
    }
}

/// Over the static map view (z 41), under the panel (42) and the status bar (43).
const Z_TILES: f32 = 41.2;
/// Map objects: OBJ priority 3, over the map (BG1) and under Mario.
const Z_OBJECTS: f32 = 41.4;
const Z_MARIO: f32 = 41.5;
const Z_STARS: f32 = 41.6;
const Z_IRIS: f32 = 41.8;
/// The item panel is BG0, the status bar's layer (priority 0: over the map's sprites).
const Z_PANEL: f32 = 43.0;

/// Debug: map cells to mark cleared at the start (`--cleared`).
#[derive(Resource, Default)]
pub struct ClearedSpots(pub Vec<(usize, usize)>);

/// Debug: the map's Hammer Bro on this cell (column, row).
#[derive(Resource, Default)]
pub struct MapBro(pub Option<(usize, usize)>);

/// `--map-at`: Mario's map cell at the start.
#[derive(Resource)]
pub struct MapAt(pub Option<(usize, usize)>);

#[derive(Resource)]
struct MapArt {
    /// [frame][tile id]
    tiles: Vec<Vec<Handle<Image>>>,
    /// [form cell * 2 + phase]
    mario: Vec<Handle<Image>>,
    /// Mario spinning after a game over, [phase].
    spin: Vec<Handle<Image>>,
    origin: (i32, i32),
    iris: Handle<Image>,
    /// The world-start stars, [tile].
    stars: Vec<Handle<Image>>,
    /// Map objects (Hammer Bros), [`sma4_assets::map_object_cell`].
    objects: Vec<Handle<Image>>,
    /// The metatile atlas on the CPU side, for the Warp Whistle's wave, and its picture.
    atlas: sma4_assets::Rgba,
    wave: Handle<Image>,
    /// The item panel (`map/item_panel.png`) and the items ([0] icons, [1] the cursor's;
    /// [0] also holds the whirlwind after the items).
    panel: Option<Handle<Image>>,
    items: Vec<Vec<Handle<Image>>>,
}

#[derive(Component)]
struct MapObjectSprite(usize);

#[derive(Component)]
struct MapTile(usize);

#[derive(Component)]
struct MapMario;

#[derive(Component)]
struct Iris;

#[derive(Component)]
struct MapStar(usize);

/// Camera-local transform of something `w` x `h` whose top-left is at GBA screen (x, y).
fn at(x: i32, y: i32, w: i32, h: i32, z: f32) -> Transform {
    Transform::from_xyz(
        x as f32 + w as f32 / 2.0 - SCREEN_W as f32 / 2.0,
        SCREEN_H as f32 / 2.0 - y as f32 - h as f32 / 2.0,
        z,
    )
}

/// Everything a world's map spawns, despawned when the Warp Whistle changes worlds.
#[derive(Component)]
struct WorldEntity;

/// The Warp Whistle: the wavy map (a picture made each frame), the whirlwind, and the
/// item panel's parts.
#[derive(Component)]
struct WaveView;
#[derive(Component)]
struct Whirlwind;
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum PanelPart {
    Panel,
    Icon(usize),
    Cursor,
}

#[allow(clippy::too_many_arguments)]
fn spawn_map(
    mut commands: Commands,
    dir: Res<crate::AssetDir>,
    mut game: ResMut<GameRes>,
    mut images: ResMut<Assets<Image>>,
    camera: Single<Entity, With<Camera2d>>,
    cleared: Option<Res<ClearedSpots>>,
    bro: Option<Res<MapBro>>,
    start_at: Option<Res<MapAt>>,
) {
    let Some(mut map) = build_world(&mut commands, &dir.0, 0, &mut images, *camera) else { return };
    for &(c, r) in cleared.iter().flat_map(|c| c.0.iter()) {
        map.clear_cell(c, r);
    }
    if let Some((c, r)) = bro.and_then(|b| b.0) {
        map.objects.place(sma4_core::map_objects::HAMMER_BRO, (r * 16 + 0x10) as u8, (c * 16) as u16);
    }
    if let Some((c, r)) = start_at.and_then(|a| a.0) {
        map.y = (r * 16 + 0x10) as u8;
        map.x = (c * 16) as u16;
    }
    game.0.world_map = Some(map);
}

/// The Warp Whistle reached World 9 (`Game::pending_world`): its map replaces World 1's.
fn reload_world(
    mut commands: Commands,
    dir: Res<crate::AssetDir>,
    mut game: ResMut<GameRes>,
    mut images: ResMut<Assets<Image>>,
    camera: Single<Entity, With<Camera2d>>,
    old: Query<Entity, With<WorldEntity>>,
) {
    let Some(world) = game.0.pending_world.take() else { return };
    for e in &old {
        commands.entity(e).despawn();
    }
    match build_world(&mut commands, &dir.0, world, &mut images, *camera) {
        Some(map) => game.0.enter_world(map),
        None => warn!("no map for world {} (re-run sma4-extract)", world + 1),
    }
}

/// Loads world `world`'s map: its entities under the camera, `MapArt`, and the core map.
fn build_world(
    commands: &mut Commands,
    dir: &std::path::Path,
    world: u8,
    images: &mut Assets<Image>,
    camera: Entity,
) -> Option<WorldMap> {
    let asset = match WorldMapAsset::read(dir, world) {
        Ok(a) => a,
        Err(e) => {
            warn!("{e}; re-run sma4-extract for the walkable world map");
            return None;
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
    let spin = if asset.mario_spin.width == 0 {
        warn!("no map/world1_mario_spin.png; re-run sma4-extract");
        vec![]
    } else {
        asset.mario_spin.cells(MAP_SPIN_SIZE, MAP_SPIN_SIZE).into_iter().map(|c| images.add(sprite_image(c))).collect()
    };
    let stars = if asset.stars.width == 0 {
        warn!("no map/world1_stars.png; re-run sma4-extract");
        vec![]
    } else {
        asset.stars.cells(MAP_STAR_SIZE, MAP_STAR_SIZE).into_iter().map(|c| images.add(sprite_image(c))).collect()
    };
    let iris = images.add(Image::new(
        Extent3d { width: SCREEN_W, height: SCREEN_H, depth_or_array_layers: 1 },
        TextureDimension::D2,
        vec![0; (SCREEN_W * SCREEN_H * 4) as usize],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    let objects_asset = match sma4_assets::MapObjectsAsset::read(dir, world) {
        Ok(a) => Some(a),
        Err(e) => {
            // World 9 (the Warp Zone) has none.
            if world == 0 {
                warn!("{e}; re-run sma4-extract for the map's Hammer Bros");
            }
            None
        }
    };
    let objects = objects_asset
        .as_ref()
        .map(|a| {
            let s = sma4_assets::MAP_OBJECT_SIZE;
            a.sprites.cells(s, s).into_iter().map(|c| images.add(sprite_image(c))).collect()
        })
        .unwrap_or_default();
    let origin = asset.origin;
    let wave = images.add(Image::new(
        Extent3d { width: SCREEN_W, height: SCREEN_H, depth_or_array_layers: 1 },
        TextureDimension::D2,
        vec![0; (SCREEN_W * SCREEN_H * 4) as usize],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    let load = |name: &str| {
        sma4_assets::Rgba::read_png(&sma4_assets::item_panel_path(dir, name))
            .map_err(|e| warn!("{e}; re-run sma4-extract for the item panel"))
            .ok()
    };
    let panel = load("panel").map(|p| images.add(sprite_image(p)));
    let items = load("items")
        .map(|sheet| {
            (0..2)
                .map(|row| {
                    let o = (row * 16 * sheet.width * 4) as usize;
                    let r = sma4_assets::Rgba {
                        width: sheet.width,
                        height: 16,
                        pixels: sheet.pixels[o..o + (16 * sheet.width * 4) as usize].to_vec(),
                    };
                    r.cells(16, 16).into_iter().map(|c| images.add(sprite_image(c))).collect()
                })
                .collect()
        })
        .unwrap_or_default();
    commands.entity(camera).with_children(|cam| {
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
                cam.spawn((WorldEntity, MapTile(r * asset.cols + c), sprite, t, Visibility::Hidden));
            }
        }
        cam.spawn((WorldEntity, MapMario, Sprite::default(), Transform::default(), Visibility::Hidden));
        cam.spawn((WorldEntity, Iris, Sprite::from_image(iris.clone()), at(0, 0, 240, 160, Z_IRIS), Visibility::Hidden));
        for i in 0..8 {
            cam.spawn((WorldEntity, MapStar(i), Sprite::default(), Transform::default(), Visibility::Hidden));
        }
        for i in 0..sma4_core::map_objects::SLOTS {
            cam.spawn((WorldEntity, MapObjectSprite(i), Sprite::default(), Transform::default(), Visibility::Hidden));
        }
        cam.spawn((WorldEntity, WaveView, Sprite::from_image(wave.clone()), at(0, 0, 240, 160, Z_TILES + 0.05), Visibility::Hidden));
        cam.spawn((WorldEntity, Whirlwind, Sprite::default(), Transform::default(), Visibility::Hidden));
        cam.spawn((WorldEntity, PanelPart::Panel, Sprite::default(), Transform::default(), Visibility::Hidden));
        for i in 0..sma4_core::inventory::SLOTS {
            cam.spawn((WorldEntity, PanelPart::Icon(i), Sprite::default(), Transform::default(), Visibility::Hidden));
        }
        cam.spawn((WorldEntity, PanelPart::Cursor, Sprite::default(), Transform::default(), Visibility::Hidden));
    });
    let map =
        WorldMap::new(asset.grid, (asset.cols, asset.rows), asset.start, asset.thresholds, asset.nodes, asset.anim_ticks)
            .with_game_over_keep(asset.game_over_keep)
            .with_origin(origin)
            .with_objects(objects_asset.as_ref().map(|a| a.objects.as_slice()).unwrap_or(&[]));
    let atlas = asset.atlas;
    commands.insert_resource(MapArt { tiles, mario, spin, origin, iris, stars, objects, atlas, wave, panel, items });
    Some(map)
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

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sync_map(
    game: Res<GameRes>,
    art: Option<Res<MapArt>>,
    mut images: ResMut<Assets<Image>>,
    mut tiles: Query<(&MapTile, &mut Sprite, &mut Visibility), (Without<MapMario>, Without<Iris>)>,
    mut mario: Query<(&mut Sprite, &mut Transform, &mut Visibility), (With<MapMario>, Without<Iris>)>,
    mut iris: Query<&mut Visibility, (With<Iris>, Without<MapTile>, Without<MapMario>)>,
    mut stars: Query<
        (&MapStar, &mut Sprite, &mut Transform, &mut Visibility),
        (Without<MapTile>, Without<MapMario>, Without<Iris>),
    >,
    mut objects: Query<
        (&MapObjectSprite, &mut Sprite, &mut Transform, &mut Visibility),
        (Without<MapTile>, Without<MapMario>, Without<Iris>, Without<MapStar>),
    >,
) {
    let (Some(art), Some(map)) = (art, &game.0.world_map) else { return };
    let menu = match game.0.screen {
        Screen::GameOverMenu(m) => Some(m),
        _ => None,
    };
    let shown = game.0.screen == Screen::Map || menu.is_some();
    let vis = |on: bool| if on { Visibility::Inherited } else { Visibility::Hidden };
    let frame = map.anim_frame as usize;
    // 0x08012458 halves the palette under the menu (BLDY); the dimming goes once the
    // panel's window has closed.
    let tint = if menu.is_some_and(|m| m.dimmed()) { Color::srgb(0.5, 0.5, 0.5) } else { Color::WHITE };
    for (tile, mut sprite, mut v) in &mut tiles {
        *v = vis(shown);
        if shown {
            sprite.color = tint;
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
        match menu.map(|m| m.mario()) {
            None => {}
            Some(None) => {
                *v = Visibility::Hidden;
                continue;
            }
            // Lower OBJ only: the spin replaces it (the small form has no upper half).
            Some(Some(MenuMario::Spin(phase))) => {
                *v = Visibility::Inherited;
                if let Some(image) = art.spin.get(phase as usize) {
                    sprite.image = image.clone();
                }
                let size = MAP_SPIN_SIZE as i32;
                *t = at(sx, sy, size, size, Z_MARIO);
                continue;
            }
            Some(Some(MenuMario::Normal)) => {}
        }
        // A world start: the stars bring him in (0x0800EA10).
        *v = vis(shown && !map.mario_hidden() && !game.0.warp.is_some_and(|w| w.mario_hidden || w.black()));
        let form = match game.0.player.form {
            Form::Small => 0,
            // TODO(re): Fire Mario's own map sprite (not extracted yet).
            Form::Big | Form::Fire => 1,
            Form::Raccoon => 2,
        };
        let phase = (game.0.frame >> 3 & 1) as usize;
        if let Some(image) = art.mario.get(form * 2 + phase) {
            sprite.image = image.clone();
        }
        *t = at(sx, sy - 16, MAP_MARIO_W as i32, MAP_MARIO_H as i32, Z_MARIO);
    }
    // The stars: 16x16 OBJs at their byte-sized screen positions, cut at the GBA screen.
    let drawn: Vec<(i32, i32, usize)> = map.stars.iter().flat_map(|s| s.stars()).collect();
    for (star, mut sprite, mut t, mut v) in &mut stars {
        let Some(&(x, y, cell)) = drawn.get(star.0).filter(|_| shown) else {
            *v = Visibility::Hidden;
            continue;
        };
        let Some(image) = art.stars.get(cell) else { continue };
        // OAM y 160-255 is above the screen.
        let y = if y >= SCREEN_H as i32 { y - 256 } else { y };
        let size = MAP_STAR_SIZE as i32;
        let (x0, x1) = (x.max(0), (x + size).min(SCREEN_W as i32));
        let (y0, y1) = (y.max(0), (y + size).min(SCREEN_H as i32));
        if x0 >= x1 || y0 >= y1 {
            *v = Visibility::Hidden;
            continue;
        }
        sprite.image = image.clone();
        sprite.rect = Some(Rect::new((x0 - x) as f32, (y0 - y) as f32, (x1 - x) as f32, (y1 - y) as f32));
        *t = at(x0, y0, x1 - x0, y1 - y0, Z_STARS);
        *v = Visibility::Inherited;
    }
    // Map objects (0x08031DDC): 16x16 OBJs placed as Mario's lower half, under him.
    let drawn = map.object_sprites();
    for (o, mut sprite, mut t, mut v) in &mut objects {
        let Some(d) = drawn.get(o.0).filter(|_| shown) else {
            *v = Visibility::Hidden;
            continue;
        };
        let Some(image) = art.objects.get(sma4_assets::map_object_cell(d.kind, d.frame) as usize) else {
            *v = Visibility::Hidden;
            continue;
        };
        sprite.image = image.clone();
        sprite.color = tint;
        sprite.flip_x = d.flip;
        let size = sma4_assets::MAP_OBJECT_SIZE as i32;
        *t = at(d.x + art.origin.0, d.y - 0x10 + art.origin.1, size, size, Z_OBJECTS);
        *v = Visibility::Inherited;
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

/// The Warp Whistle and the item panel over the map.
#[allow(clippy::type_complexity)]
fn sync_warp(
    game: Res<GameRes>,
    art: Option<Res<MapArt>>,
    mut images: ResMut<Assets<Image>>,
    mut wave: Query<&mut Visibility, (With<WaveView>, Without<Whirlwind>, Without<PanelPart>)>,
    mut whirl: Query<(&mut Sprite, &mut Transform, &mut Visibility), (With<Whirlwind>, Without<WaveView>, Without<PanelPart>)>,
    mut parts: Query<(&PanelPart, &mut Sprite, &mut Transform, &mut Visibility), (Without<WaveView>, Without<Whirlwind>)>,
) {
    let (Some(art), Some(map)) = (art, &game.0.world_map) else { return };
    let on_map = game.0.screen == Screen::Map;
    let warp = game.0.warp.filter(|_| on_map);
    // The wave: the map redrawn line by line, each shifted (black past the map's edges).
    let waving = warp.is_some_and(|w| w.amp > 0 || w.black());
    for mut v in &mut wave {
        *v = if waving { Visibility::Inherited } else { Visibility::Hidden };
    }
    if let Some(w) = warp.filter(|_| waving)
        && let Some(mut image) = images.get_mut(&art.wave)
        && let Some(data) = image.data.as_mut()
    {
        let frame = map.anim_frame as i32;
        let a = &art.atlas;
        for y in 0..SCREEN_H as i32 {
            let shift = w.shift(y);
            for x in 0..SCREEN_W as i32 {
                let (mx, my) = (x + shift - art.origin.0, y - art.origin.1);
                let (c, r) = (mx.div_euclid(16), my.div_euclid(16));
                let o = ((y * SCREEN_W as i32 + x) * 4) as usize;
                let inside = !w.black() && (0..map.cols as i32).contains(&c) && (0..map.rows as i32).contains(&r);
                if !inside {
                    data[o..o + 4].copy_from_slice(&[0, 0, 0, 255]);
                    continue;
                }
                let id = map.tile(c, r) as i32;
                let (ax, ay) = (frame * 16 + mx.rem_euclid(16), id * 16 + my.rem_euclid(16));
                let s = ((ay * a.width as i32 + ax) * 4) as usize;
                data[o..o + 4].copy_from_slice(&a.pixels[s..s + 4]);
            }
        }
    }
    // The whirlwind: a 16x16 OBJ on Mario's row (0x080117B4), mirrored coming from the right.
    for (mut sprite, mut t, mut v) in &mut whirl {
        let shown = warp.and_then(|w| w.whirlwind().map(|(x, flip)| (w, x, flip)));
        let Some((w, x, flip)) = shown else {
            *v = Visibility::Hidden;
            continue;
        };
        let cell = sma4_assets::ITEM_KINDS as usize + w.whirlwind_frame();
        let Some(image) = art.items.first().and_then(|r| r.get(cell)) else { continue };
        sprite.image = image.clone();
        sprite.flip_x = flip;
        let y = w.row as i32 + map.sprite_origin.1;
        let x = if x >= 0xF0 { x - 0x100 } else { x };
        *t = at(x, y, 16, 16, Z_MARIO + 0.5);
        *v = Visibility::Inherited;
    }
    // The item panel: BG0's line L at screen y L - VOFS; the icons in it, the cursor over.
    let panel = game.0.item_panel.as_ref().filter(|p| on_map && p.panel_map);
    for (part, mut sprite, mut t, mut v) in &mut parts {
        let Some(p) = panel else {
            *v = Visibility::Hidden;
            continue;
        };
        let top = -(p.vofs as i32);
        let placed = match *part {
            PanelPart::Panel => art.panel.clone().map(|image| (image, 0, top, 256, 256)),
            PanelPart::Icon(i) => game.0.inventory.get(i).filter(|&&it| it != 0).and_then(|&it| {
                let (r, c) = ((i / sma4_core::inventory::PER_ROW) as i32, (i % sma4_core::inventory::PER_ROW) as i32);
                let (x, y) = (sma4_core::inventory::CURSOR_X0 + c * sma4_core::inventory::CURSOR_DX, top + sma4_core::inventory::ICON_LINE0 + 24 * r);
                art.items.first().and_then(|row| row.get(it as usize)).map(|im| (im.clone(), x, y, 16, 16))
            }),
            PanelPart::Cursor => (p.state == sma4_core::inventory::State::Menu)
                .then(|| game.0.inventory.get(p.slot()).copied())
                .flatten()
                .and_then(|it| art.items.get(1).and_then(|row| row.get(it as usize)))
                .map(|im| {
                    let x = sma4_core::inventory::CURSOR_X0 + p.col as i32 * sma4_core::inventory::CURSOR_DX;
                    (im.clone(), x, sma4_core::inventory::CURSOR_Y[p.row], 16, 16)
                }),
        };
        let Some((image, x, y, w, h)) = placed else {
            *v = Visibility::Hidden;
            continue;
        };
        // Cut at the GBA screen.
        let (x0, x1) = (x.max(0), (x + w).min(SCREEN_W as i32));
        let (y0, y1) = (y.max(0), (y + h).min(SCREEN_H as i32));
        if x0 >= x1 || y0 >= y1 {
            *v = Visibility::Hidden;
            continue;
        }
        sprite.image = image;
        sprite.rect = Some(Rect::new((x0 - x) as f32, (y0 - y) as f32, (x1 - x) as f32, (y1 - y) as f32));
        let z = match *part {
            PanelPart::Panel => Z_PANEL,
            PanelPart::Icon(_) => Z_PANEL + 0.1,
            PanelPart::Cursor => Z_PANEL + 0.2,
        };
        *t = at(x0, y0, x1 - x0, y1 - y0, z);
        *v = Visibility::Inherited;
    }
}

//! The world-map and game-over screens, drawn over the GBA's 240x160 screen from the
//! assets `sma4-extract` built by running the ROM's own screen setup (`map/world1*`,
//! `screens/game_over.png`). What changes on them comes from `sma4_core`.

use bevy::prelude::*;
use sma4_assets::{HudAsset, MapScreenAsset, Rgba, game_over_cursor_path, game_over_menu_path, game_over_path};
use sma4_core::screens::{GAME_OVER_HEAD_X, GAME_OVER_Y};
use sma4_core::{SCREEN_H, SCREEN_W, Screen};

use crate::GameRes;
use crate::player::{load_cells, sprite_image};

pub struct ScreensPlugin;

impl Plugin for ScreensPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, spawn_screens).add_systems(Update, (sync_map, sync_game_over, sync_menu));
    }
}

// Above the level and its sprites; the in-level status bar (z 50) is hidden off-level.
const Z_COVER: f32 = 40.0;
const Z_MAP: f32 = 41.0;
const Z_PANEL: f32 = 42.0;
const Z_BAR: f32 = 43.0;
const Z_SPRITES: f32 = 44.0;

#[derive(Resource)]
struct ScreenArt {
    map: Option<MapScreenAsset>,
    hud: Option<HudAsset>,
    tiles: Vec<Handle<Image>>,
}

/// Everything shown only on the map.
#[derive(Component)]
struct OnMap;

/// The static map picture, used when the walkable map is not extracted.
#[derive(Component)]
struct MapView;

/// The panel's sprites (Mario's face next to "MARIO").
#[derive(Component)]
struct PanelSprites;

/// A map-screen BG cell: layer (0 bar, 3 panel) and index in its 32x32 map.
#[derive(Component)]
struct MapCell(u8, usize);

#[derive(Component)]
struct OffLevelCover;

#[derive(Component)]
struct GameOverLetter(usize);

#[derive(Component)]
struct GameOverHead;

/// The game-over menu screen and its cursor.
#[derive(Component)]
struct MenuScreen;

#[derive(Component)]
struct MenuCursor;

/// Camera-local position of something `w` x `h` whose top-left is at GBA screen (x, y).
fn at(x: i32, y: i32, w: i32, h: i32, z: f32) -> Transform {
    Transform::from_xyz(
        (x + w / 2) as f32 - SCREEN_W as f32 / 2.0,
        SCREEN_H as f32 / 2.0 - (y + h / 2) as f32,
        z,
    )
}

/// Where a cell of a 32x32 BG map lands on screen with the layer's scroll, if visible.
fn cell_on_screen(index: usize, scroll: (u16, u16)) -> Option<(i32, i32)> {
    let (c, r) = ((index % 32) as i32, (index / 32) as i32);
    let x = (c * 8 - scroll.0 as i32).rem_euclid(256);
    let y = (r * 8 - scroll.1 as i32).rem_euclid(256);
    (x < SCREEN_W as i32 && y < SCREEN_H as i32).then_some((x, y))
}

fn spawn_screens(
    mut commands: Commands,
    dir: Res<crate::AssetDir>,
    mut images: ResMut<Assets<Image>>,
    camera: Single<Entity, With<Camera2d>>,
) {
    let map = MapScreenAsset::read(&dir.0, 0).map_err(|e| warn!("{e}; re-run sma4-extract")).ok();
    let hud = HudAsset::read(&dir.0).ok();
    let tiles = map
        .as_ref()
        .map(|m| m.tiles.cells(8, 8).into_iter().map(|c| images.add(sprite_image(c))).collect())
        .unwrap_or_default();
    let letters = load_cells(&game_over_path(&dir.0), 8, 16, &mut images);
    let head = Rgba::read_png(&game_over_path(&dir.0)).ok().map(|s| {
        let cells = s.cells(16, 16);
        images.add(sprite_image(cells[5].clone()))
    });

    commands.entity(*camera).with_children(|cam| {
        // Covers the level (and any view area beyond the GBA screen) off-level.
        cam.spawn((
            OffLevelCover,
            Sprite { color: Color::BLACK, custom_size: Some(Vec2::splat(4096.0)), ..default() },
            Transform::from_xyz(0.0, 0.0, Z_COVER),
            Visibility::Hidden,
        ));
        if let Some(m) = &map {
            let view = images.add(sprite_image(m.map.clone()));
            cam.spawn((OnMap, MapView, Sprite::from_image(view), at(0, 0, 240, 160, Z_MAP), Visibility::Hidden));
            let sprites = images.add(sprite_image(m.sprites.clone()));
            cam.spawn((OnMap, PanelSprites, Sprite::from_image(sprites), at(0, 0, 240, 160, Z_SPRITES), Visibility::Hidden));
            for (layer, scroll, z) in [(0u8, m.bar_scroll, Z_BAR), (3, m.panel_scroll, Z_PANEL)] {
                for i in 0..32 * 32 {
                    if let Some((x, y)) = cell_on_screen(i, scroll) {
                        cam.spawn((OnMap, MapCell(layer, i), Sprite::default(), at(x, y, 8, 8, z), Visibility::Hidden));
                    }
                }
            }
        }
        for (i, image) in letters.iter().take(10).enumerate() {
            cam.spawn((GameOverLetter(i), Sprite::from_image(image.clone()), Transform::default(), Visibility::Hidden));
        }
        if let Some(head) = head {
            cam.spawn((GameOverHead, Sprite::from_image(head), Transform::default(), Visibility::Hidden));
        }
        if let Ok(menu) = Rgba::read_png(&game_over_menu_path(&dir.0)) {
            let image = images.add(sprite_image(menu));
            cam.spawn((MenuScreen, Sprite::from_image(image), at(0, 0, 240, 160, Z_MAP), Visibility::Hidden));
        }
        if let Ok(cursor) = Rgba::read_png(&game_over_cursor_path(&dir.0)) {
            let image = images.add(sprite_image(cursor));
            cam.spawn((MenuCursor, Sprite::from_image(image), Transform::default(), Visibility::Hidden));
        }
    });
    commands.insert_resource(ScreenArt { map, hud, tiles });
}

fn sync_map(
    game: Res<GameRes>,
    art: Res<ScreenArt>,
    mut cover: Query<&mut Visibility, (With<OffLevelCover>, Without<OnMap>)>,
    mut parts: Query<(&mut Visibility, Option<&MapCell>, Has<MapView>, &mut Sprite), With<OnMap>>,
) {
    let off_level = game.0.screen != Screen::Level;
    for mut v in &mut cover {
        *v = if off_level { Visibility::Inherited } else { Visibility::Hidden };
    }
    let on_map = game.0.screen == Screen::Map;
    // The game-over menu sits on the map screen too and keeps its status bar.
    let bar_shown = on_map || matches!(game.0.screen, Screen::GameOverMenu(_));
    let (Some(m), Some(hud)) = (&art.map, &art.hud) else { return };
    let (bar, panel) =
        if bar_shown { (game.0.status.map_bar(&m.bar, hud), game.0.status.map_panel(&m.panel, hud)) } else { (vec![], vec![]) };
    // With the walkable map, the static picture is not needed and the "WORLD n" panel only
    // shows during the map intro.
    let walkable = game.0.world_map.as_ref();
    let panel_shown = on_map && walkable.is_none_or(|m| matches!(m.phase, sma4_core::MapPhase::Intro(_)));
    for (mut vis, cell, is_view, mut sprite) in &mut parts {
        let shown = match cell {
            Some(&MapCell(0, _)) => bar_shown,
            _ if is_view => on_map && walkable.is_none(),
            _ => panel_shown,
        };
        if !shown {
            *vis = Visibility::Hidden;
            continue;
        }
        let Some(&MapCell(layer, i)) = cell else {
            *vis = Visibility::Inherited;
            continue;
        };
        let entry = if layer == 0 { bar[i] } else { panel[i] };
        match m.cell(layer, entry).and_then(|c| art.tiles.get(c)) {
            Some(image) => {
                if sprite.image != *image {
                    sprite.image = image.clone();
                }
                sprite.flip_x = entry & 0x400 != 0;
                sprite.flip_y = entry & 0x800 != 0;
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
}

fn sync_game_over(
    game: Res<GameRes>,
    mut letters: Query<(&GameOverLetter, &mut Transform, &mut Visibility), Without<GameOverHead>>,
    mut head: Query<(&mut Transform, &mut Visibility), With<GameOverHead>>,
) {
    let Screen::GameOver(g) = game.0.screen else {
        for (_, _, mut v) in &mut letters {
            *v = Visibility::Hidden;
        }
        for (_, mut v) in &mut head {
            *v = Visibility::Hidden;
        }
        return;
    };
    let xs = g.letter_x();
    for (l, mut t, mut v) in &mut letters {
        *t = at(xs[l.0], GAME_OVER_Y, 8, 16, Z_SPRITES);
        *v = Visibility::Inherited;
    }
    for (mut t, mut v) in &mut head {
        *t = at(GAME_OVER_HEAD_X, g.head_y as i32, 16, 16, Z_SPRITES + 1.0);
        *v = Visibility::Inherited;
    }
}

fn sync_menu(
    game: Res<GameRes>,
    mut screen: Query<(&mut Sprite, &mut Transform, &mut Visibility), (With<MenuScreen>, Without<MenuCursor>)>,
    mut cursor: Query<(&mut Transform, &mut Visibility), (With<MenuCursor>, Without<MenuScreen>)>,
) {
    let menu = match game.0.screen {
        Screen::GameOverMenu(m) => Some(m),
        _ => None,
    };
    for (mut sprite, mut t, mut v) in &mut screen {
        // With the walkable map drawn (dimmed) underneath, only the panel's window of the
        // menu picture is shown (0x0802F718: it closes after CONTINUE).
        if game.0.world_map.is_some() {
            match menu.and_then(|m| m.window()) {
                Some((x, y, w, h)) => {
                    sprite.rect = Some(Rect::new(x as f32, y as f32, (x + w) as f32, (y + h) as f32));
                    *t = at(x, y, w, h, Z_PANEL);
                    *v = Visibility::Inherited;
                }
                None => *v = Visibility::Hidden,
            }
            continue;
        }
        *v = if menu.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    }
    for (mut t, mut v) in &mut cursor {
        match menu.and_then(|m| m.cursor()) {
            Some((x, y)) => {
                *t = at(x, y, 8, 8, Z_SPRITES);
                *v = Visibility::Inherited;
            }
            None => *v = Visibility::Hidden,
        }
    }
}

//! The status bar and score popups, drawn from the ROM's own status-bar map and tiles
//! (`hud/status_map.bin`, `hud/status_tiles.png`, `objects/score_popups.png`). What goes
//! into the map each frame comes from `sma4_core::status`.

use bevy::prelude::*;
use sma4_assets::{HUD_COLS, HUD_ROWS, HUD_TILES, HudAsset, SCORE_POPUP_TYPES, hud_cell};
use sma4_core::status::{POPUP_1UP, life_popup_scale};
use sma4_core::{SCREEN_H, SCREEN_W};

use crate::GameRes;
use crate::player::load_cells;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, spawn_status_bar)
            .add_systems(Update, sync_popups)
            .add_systems(PostUpdate, sync_status_bar.before(TransformSystems::Propagate));
    }
}

/// Above everything in the level.
const Z_HUD: f32 = 50.0;
const Z_POPUP: f32 = 12.0;
/// Rows of the 20-row map that hold anything: the top line and the P-meter.
const ROWS_USED: [usize; 3] = [0, 18, 19];

#[derive(Resource)]
struct HudArt {
    asset: Option<HudAsset>,
    tiles: Vec<Handle<Image>>,
    popups: Vec<Handle<Image>>,
    lives: Vec<Handle<Image>>,
}

/// One status-bar cell (map index).
#[derive(Component)]
struct HudCell(usize);

#[derive(Component)]
struct PopupSprite;

fn spawn_status_bar(
    mut commands: Commands,
    dir: Res<crate::AssetDir>,
    mut images: ResMut<Assets<Image>>,
    camera: Single<Entity, With<Camera2d>>,
) {
    let asset = HudAsset::read(&dir.0).map_err(|e| warn!("{e}; re-run sma4-extract")).ok();
    let tiles = load_cells(&HudAsset::tiles_path(&dir.0), 8, 8, &mut images);
    let popups = load_cells(&sma4_assets::score_popups_path(&dir.0), 16, 8, &mut images);
    let lives = load_cells(&sma4_assets::life_popups_path(&dir.0), 32, 16, &mut images);
    debug_assert!(tiles.is_empty() || tiles.len() == 2 * HUD_TILES);
    debug_assert!(popups.is_empty() || popups.len() == SCORE_POPUP_TYPES);
    // Cells are children of the camera, laid out over the GBA's 240x160 screen centred in
    // the view (30 visible columns of the 32-column map).
    commands.entity(*camera).with_children(|cam| {
        for row in ROWS_USED {
            for col in 0..SCREEN_W as usize / 8 {
                let x = col as f32 * 8.0 + 4.0 - SCREEN_W as f32 / 2.0;
                let y = SCREEN_H as f32 / 2.0 - row as f32 * 8.0 - 4.0;
                cam.spawn((
                    HudCell(row * HUD_COLS + col),
                    Sprite::default(),
                    Transform::from_xyz(x, y, Z_HUD),
                    Visibility::Hidden,
                ));
            }
        }
    });
    debug_assert_eq!(HUD_ROWS, 20);
    commands.insert_resource(HudArt { asset, tiles, popups, lives });
}

fn sync_status_bar(
    game: Res<GameRes>,
    art: Res<HudArt>,
    projection: Single<&Projection, With<Camera2d>>,
    mut cells: Query<(&HudCell, &mut Sprite, &mut Visibility, &mut Transform)>,
) {
    let Some(asset) = &art.asset else { return };
    // Only in the level; the map screen has its own bar.
    if game.0.screen != sma4_core::Screen::Level {
        for (_, _, mut vis, _) in &mut cells {
            *vis = Visibility::Hidden;
        }
        return;
    }
    let map = game.0.status.map(asset, game.0.p_meter_display());
    // The view can be taller than the GBA screen: keep the top line at the top edge and
    // the P-meter rows at the bottom edge.
    let half_h = match *projection {
        Projection::Orthographic(o) => o.area.height() / 2.0,
        _ => SCREEN_H as f32 / 2.0,
    };
    for (cell, mut sprite, mut vis, mut transform) in &mut cells {
        let row = cell.0 / HUD_COLS;
        transform.translation.y = if row < HUD_ROWS / 2 {
            half_h - row as f32 * 8.0 - 4.0
        } else {
            -half_h + (HUD_ROWS - row) as f32 * 8.0 - 4.0
        };
        match hud_cell(map[cell.0]).and_then(|i| art.tiles.get(i)) {
            Some(image) => {
                if sprite.image != *image {
                    sprite.image = image.clone();
                }
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
}

fn sync_popups(mut commands: Commands, game: Res<GameRes>, art: Res<HudArt>, old: Query<Entity, With<PopupSprite>>) {
    for e in &old {
        commands.entity(e).despawn();
    }
    if game.0.screen != sma4_core::Screen::Level {
        return;
    }
    for p in &game.0.status.popups {
        let life = p.kind >= POPUP_1UP;
        let image = if life { art.lives.get((p.kind - POPUP_1UP) as usize) } else { art.popups.get(p.kind as usize - 1) };
        let Some(image) = image else { continue };
        let (w, h) = if life { (32, 16) } else { (16, 8) };
        // The extra-life popup zooms in (affine scale: 0x100 = full size).
        let scale = if life { 256.0 / life_popup_scale(p.timer) as f32 } else { 1.0 };
        commands.spawn((
            PopupSprite,
            Sprite::from_image(image.clone()),
            Transform::from_xyz((p.x + w / 2) as f32, -((p.y + h / 2) as f32), Z_POPUP)
                .with_scale(Vec3::new(scale, scale, 1.0)),
        ));
    }
}

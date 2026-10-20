//! Player sprite, HUD and camera. The frames come from the user's own ROM via
//! `sma4-extract` (`objects/player_small.png`, `objects/player_big.png`).

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use sma4_assets::{PLAYER_BODY_W, PLAYER_H, PLAYER_W, Rgba};
use sma4_core::{Form, SCREEN_H, SCREEN_W};

use crate::GameRes;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_player)
            .add_systems(Update, (debug_hurt, sync_sprite, follow_camera.in_set(CameraMoved)).chain());
    }
}

/// Sprite image from an extracted RGBA cell.
pub fn sprite_image(cell: Rgba) -> Image {
    Image::new(
        Extent3d { width: cell.width, height: cell.height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        cell.pixels,
        TextureFormat::Rgba8UnormSrgb,
        // Sprites need the image in the main world too (for their size).
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
}

/// Loads a sheet of `w` x `h` cells; empty (with a warning) if it is missing.
/// The camera has followed the player this frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CameraMoved;

pub fn load_cells(path: &std::path::Path, w: u32, h: u32, images: &mut Assets<Image>) -> Vec<Handle<Image>> {
    match Rgba::read_png(path) {
        Ok(sheet) => sheet.cells(w, h).into_iter().map(|c| images.add(sprite_image(c))).collect(),
        Err(e) => {
            warn!("{e}; re-run sma4-extract");
            Vec::new()
        }
    }
}

/// Per form: one cell per pose slot (`sma4_assets::PLAYER_*`).
#[derive(Resource)]
struct Frames {
    small: Vec<Handle<Image>>,
    big: Vec<Handle<Image>>,
    raccoon: Vec<Handle<Image>>,
    /// Grow / shrink steps (`player_grow.png`) and the smoke puff (`puff.png`).
    grow: Vec<Handle<Image>>,
    puff: Vec<Handle<Image>>,
}

/// Puff tiles 0x0817388C[i] = 0x83, 0x82, 0x81, 0x80 as cells of `puff.png` (which holds
/// tiles 0x81, 0x80, 0x82, 0x83).
const PUFF_CELLS: [usize; 4] = [3, 2, 0, 1];

#[derive(Component)]
struct PlayerSprite;

/// Behind the level's tilemaps (z 0), over the background scene.
pub(crate) const Z_BEHIND_TILES: f32 = -2.0;

fn spawn_player(mut commands: Commands, dir: Res<crate::AssetDir>, mut images: ResMut<Assets<Image>>) {
    let mut load = |form| load_cells(&sma4_assets::player_path(&dir.0, form), PLAYER_W, PLAYER_H, &mut images);
    let frames = Frames {
        small: load(0),
        big: load(1),
        raccoon: load(3),
        grow: load_cells(&sma4_assets::player_grow_path(&dir.0), PLAYER_W, PLAYER_H, &mut images),
        puff: load_cells(&sma4_assets::puff_path(&dir.0), 16, 16, &mut images),
    };
    commands.spawn((PlayerSprite, Sprite::default(), Transform::from_xyz(0.0, 0.0, 10.0)));
    commands.insert_resource(frames);
}

/// Debug: H simulates the player taking a hit (there are no enemies yet), so the extra
/// hit point from the growth item can be tested.
fn debug_hurt(keys: Res<ButtonInput<KeyCode>>, mut game: ResMut<GameRes>) {
    if keys.just_pressed(KeyCode::KeyH) {
        game.0.hurt();
    }
}

/// Box centre in world space (y up).
fn player_centre(game: &GameRes) -> Vec2 {
    let (cx, cy) = game.0.player.centre();
    Vec2::new(cx as f32, -(cy as f32))
}

fn sync_sprite(
    game: Res<GameRes>,
    frames: Res<Frames>,
    mut q: Single<(&mut Sprite, &mut Transform), With<PlayerSprite>>,
) {
    let p = &game.0.player;
    let (sprite, transform) = &mut *q;
    let set = match p.form {
        Form::Small => &frames.small,
        Form::Big => &frames.big,
        Form::Raccoon => &frames.raccoon,
    };
    // The cell's bottom row is the feet; its left 16 columns are the body, centred on the
    // box, and the raccoon tail extends into the right half. The art faces left; facing
    // right, the ROM h-flips the 32x32 sprite and moves it 16 px left (0x080A6E0C).
    let (cx, top) = (p.centre().0, p.px().1);
    let feet = top + p.height();
    let half_body = PLAYER_BODY_W as i32 / 2;
    let centre_x = if p.facing_left { cx - half_body + PLAYER_W as i32 / 2 } else { cx + half_body - PLAYER_W as i32 / 2 };
    // In a pipe, the pipe's tiles cover the player (0x0809BA58).
    let z = if game.0.pipe.is_some() { Z_BEHIND_TILES } else { 10.0 };
    transform.translation = Vec3::new(centre_x as f32, -((feet - PLAYER_H as i32 / 2) as f32), z);
    sprite.flip_x = !p.facing_left;
    // Blink while invulnerable after a hit.
    let hidden = game.0.invulnerable > 0 && (game.0.invulnerable / 4) % 2 == 1;
    sprite.color = if hidden { Color::NONE } else { Color::WHITE };
    match p.pose() {
        // 0x080A90C0: only the smoke, two 8x16 halves at the frame's x, 8 px below its top.
        sma4_core::player::Pose::Puff(i) if game.0.dying.is_none() => {
            if let Some(image) = frames.puff.get(PUFF_CELLS[i as usize & 3]) {
                sprite.image = image.clone();
            }
            sprite.flip_x = false;
            let frame_top = feet - PLAYER_H as i32;
            transform.translation = Vec3::new(cx as f32, -((frame_top + 8 + 8) as f32), 10.0);
            return;
        }
        // 0x080A8000: a grow / shrink step with the big form's graphics.
        sma4_core::player::Pose::Morph(i) if game.0.dying.is_none() => {
            if let Some(image) = frames.grow.get(i as usize) {
                sprite.image = image.clone();
            }
            return;
        }
        _ => {}
    }
    // Dying: the death pose, drawn with the small form's graphics (0x080A8FC0).
    let (set, slot) = if game.0.dying.is_some() {
        sprite.flip_x = false;
        (&frames.small, sma4_assets::PLAYER_DEATH)
    } else {
        (set, p.pose().slot())
    };
    if let Some(image) = set.get(slot) {
        sprite.image = image.clone();
    }
}

/// Keeps the player in view, clamped to the level bounds, snapped to whole pixels.
/// Uses the camera's real visible area: the window may not have the GBA's aspect ratio.
fn follow_camera(
    game: Res<GameRes>,
    mut cam: Single<(&mut Transform, &Projection), (With<Camera2d>, Without<PlayerSprite>)>,
) {
    let map = &game.0.level;
    // Course clear: the scroll stops where it was when the card was touched.
    if map.width == 0 || game.0.goal_camera.is_some() {
        return;
    }
    let (transform, projection) = &mut *cam;
    let view = match projection {
        Projection::Orthographic(o) => o.area.size(),
        _ => Vec2::new(SCREEN_W as f32, SCREEN_H as f32),
    };
    let (half_w, half_h) = (view.x / 2.0, view.y / 2.0);
    let c = player_centre(&game);
    let level_w = map.pixel_width() as f32;
    let level_h = map.pixel_height() as f32;
    let mut x = c.x.clamp(half_w, (level_w - half_w).max(half_w));
    // Bottom of the level stays at the bottom of the view; taller views show more sky.
    let mut y = c.y.clamp(-(level_h - half_h), (-half_h).max(-(level_h - half_h)));
    // Areas where the ROM sets the camera limits (the coin room): the core's camera.
    let (gx, gy) = game.0.camera();
    if map.camera_x.is_some() {
        x = (gx + SCREEN_W as i32 / 2) as f32;
    }
    if map.camera_y.is_some() {
        y = -((gy + SCREEN_H as i32 / 2) as f32);
    }
    transform.translation.x = x.round();
    transform.translation.y = y.round();
}

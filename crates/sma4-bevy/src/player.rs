//! Player sprite and camera. The sprite is "Pip", an original placeholder character drawn
//! here as pixel art; it stands in for the player graphics until those are wired up.

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use sma4_core::{
    SCREEN_H, SCREEN_W,
    player::{HITBOX_H, HITBOX_W, TILE_PX},
};

use crate::GameRes;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_player)
            .add_systems(Update, (sync_sprite, follow_camera).chain());
    }
}

/// 16x16 frames. `.` transparent, `o` outline, `t` body, `l` belly, `w` eye white,
/// `k` pupil, `g` leaf, `s` stem, `f` feet.
const STAND: [&str; 16] = [
    "........g.......",
    ".......ggg......",
    "........s.......",
    ".....oooooo.....",
    "....otttttto....",
    "...otttttttto...",
    "..otttwwtwwtto..",
    "..otttwktwktto..",
    "..ottttttttto...",
    "..ottlllllltto..",
    "..otllllllllto..",
    "..otllllllllto..",
    "...otllllllto...",
    "....oooooooo....",
    "....off..ffo....",
    "....ff....ff....",
];
const STEP: [&str; 16] = [
    "........g.......",
    ".......ggg......",
    "........s.......",
    ".....oooooo.....",
    "....otttttto....",
    "...otttttttto...",
    "..otttwwtwwtto..",
    "..otttwktwktto..",
    "..ottttttttto...",
    "..ottlllllltto..",
    "..otllllllllto..",
    "..otllllllllto..",
    "...otllllllto...",
    "....oooooooo....",
    "...ff.....off...",
    "..ff.......ff...",
];
const JUMP: [&str; 16] = [
    ".......g.g......",
    "........g.......",
    "........s.......",
    ".....oooooo.....",
    "....otttttto....",
    "...otttttttto...",
    "..otttwwtwwtto..",
    "..otttwktwktto..",
    "..ottttttttto...",
    ".fottlllllltto..",
    "ff.otllllllto.ff",
    "...otllllllto..f",
    "...otllllllto...",
    "....oooooooo....",
    "................",
    "................",
];

fn colour(c: char) -> [u8; 4] {
    match c {
        'o' => [0x1d, 0x2b, 0x3a, 255],
        't' => [0x2f, 0xb5, 0xa8, 255],
        'l' => [0xd8, 0xf3, 0xe6, 255],
        'w' => [0xff, 0xff, 0xff, 255],
        'k' => [0x10, 0x10, 0x18, 255],
        'g' => [0x6c, 0xc6, 0x3c, 255],
        's' => [0x3f, 0x7a, 0x2a, 255],
        'f' => [0xf2, 0x9e, 0x38, 255],
        _ => [0, 0, 0, 0],
    }
}

fn frame_image(rows: &[&str; 16]) -> Image {
    let data: Vec<u8> = rows.iter().flat_map(|r| r.chars().flat_map(colour)).collect();
    Image::new(
        Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        // Sprites need the image in the main world too (for their size).
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
}

#[derive(Resource)]
struct Frames {
    stand: Handle<Image>,
    step: Handle<Image>,
    jump: Handle<Image>,
}

#[derive(Component)]
struct PlayerSprite;

fn spawn_player(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let frames = Frames {
        stand: images.add(frame_image(&STAND)),
        step: images.add(frame_image(&STEP)),
        jump: images.add(frame_image(&JUMP)),
    };
    commands.spawn((PlayerSprite, Sprite::from_image(frames.stand.clone()), Transform::from_xyz(0.0, 0.0, 10.0)));
    commands.insert_resource(frames);
}

/// Hitbox centre in world space (y up).
fn player_centre(game: &GameRes) -> Vec2 {
    let (px, py) = game.0.player.px();
    Vec2::new((px + HITBOX_W / 2) as f32, -((py + HITBOX_H / 2) as f32))
}

fn sync_sprite(
    game: Res<GameRes>,
    frames: Res<Frames>,
    mut q: Single<(&mut Sprite, &mut Transform), With<PlayerSprite>>,
) {
    let p = &game.0.player;
    let (sprite, transform) = &mut *q;
    let c = player_centre(&game);
    transform.translation = c.extend(10.0);
    sprite.flip_x = p.facing_left;
    sprite.image = if !p.on_ground {
        frames.jump.clone()
    } else if p.vx != 0 && (p.walk_frames / 6) % 2 == 1 {
        frames.step.clone()
    } else {
        frames.stand.clone()
    };
}

/// Keeps the player in view, clamped to the level bounds, snapped to whole pixels.
/// Uses the camera's real visible area: the window may not have the GBA's aspect ratio.
fn follow_camera(
    game: Res<GameRes>,
    mut cam: Single<(&mut Transform, &Projection), (With<Camera2d>, Without<PlayerSprite>)>,
) {
    let map = &game.0.collision;
    if map.width == 0 {
        return;
    }
    let (transform, projection) = &mut *cam;
    let view = match projection {
        Projection::Orthographic(o) => o.area.size(),
        _ => Vec2::new(SCREEN_W as f32, SCREEN_H as f32),
    };
    let (half_w, half_h) = (view.x / 2.0, view.y / 2.0);
    let c = player_centre(&game);
    let level_w = (map.width as i32 * TILE_PX) as f32;
    let level_h = map.pixel_height() as f32;
    let x = c.x.clamp(half_w, (level_w - half_w).max(half_w));
    // Bottom of the level stays at the bottom of the view; taller views show more sky.
    let y = c.y.clamp(-(level_h - half_h), (-half_h).max(-(level_h - half_h)));
    let cam = transform;
    cam.translation.x = x.round();
    cam.translation.y = y.round();
}

//! The King's castle and the airship: the objects the core draws as hardware sprites
//! (`sma4_core::obj`), rendered from the room's OBJ VRAM snapshot (`castle/obj_XXX.bin`),
//! and the throne room's message box (BG3, fixed on the screen). The rooms run in
//! `sma4-core` (`castle.rs`).

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use sma4_assets::{CASTLE_BOX_AT, CASTLE_BOX_H, CASTLE_BOX_W, CASTLE_MESSAGES, CASTLE_TEXT_AT, CastleAsset, ObjBank, Rgba};
use sma4_core::obj::ObjSprite;
use sma4_core::{SCREEN_H, SCREEN_W};

use crate::GameRes;
use crate::level::LevelRes;
use crate::player::sprite_image;

pub struct CastlePlugin;

impl Plugin for CastlePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_data).add_systems(Update, (sync_objs, sync_box, sync_curtain, sync_sky));
    }
}

/// Sprites over the level's tiles (z 0) with the enemies; priority 3 under them.
const Z_OBJ: f32 = 8.0;
/// Entries that come before the player's in OAM (the first `Game::obj_front`: Larry's wand).
const Z_OBJ_FRONT: f32 = 10.5;
const Z_OBJ_BEHIND: f32 = -0.5;
/// BG3 (priority 0): over the sprites, under the status bar (z 50).
const Z_BOX: f32 = 30.0;
/// The curtain between rooms covers everything, the status bar too; its sprites ("MARIO
/// START!") are over it.
const Z_CURTAIN: f32 = 60.0;
const Z_CURTAIN_OBJ: f32 = 61.0;

#[derive(Resource, Default)]
struct Art {
    /// The airship's sky: BG2 behind the level, BG3 in front.
    sky: Option<(Handle<Image>, Handle<Image>)>,
    /// The curtain's iris (GBA screen sized) and the radius it was drawn for.
    iris: Handle<Image>,
    iris_drawn: Option<i32>,
    message_box: Option<Handle<Image>>,
    text: Vec<(u8, Handle<Image>)>,
    /// OBJ banks by room, and the sprites rendered from them.
    banks: HashMap<u16, Option<ObjBank>>,
    cache: HashMap<(u16, ObjSprite), Handle<Image>>,
}

#[derive(Component)]
struct CastleSprite;

fn load_one(path: &std::path::Path, images: &mut Assets<Image>) -> Option<Handle<Image>> {
    match Rgba::read_png(path) {
        Ok(img) => Some(images.add(sprite_image(img))),
        Err(e) => {
            warn!("{e}; re-run sma4-extract for the King's castle");
            None
        }
    }
}

fn load_data(mut commands: Commands, dir: Res<crate::AssetDir>, mut game: ResMut<GameRes>, mut images: ResMut<Assets<Image>>) {
    match CastleAsset::read(&dir.0) {
        Ok(a) => game.0.castle_data = Some(Arc::new(a)),
        Err(e) => warn!("{e}; re-run sma4-extract for the King's castle"),
    }
    let message_box = load_one(&sma4_assets::castle_box_path(&dir.0), &mut images);
    let text = CASTLE_MESSAGES
        .iter()
        .filter_map(|&id| load_one(&sma4_assets::castle_text_path(&dir.0, id), &mut images).map(|h| (id, h)))
        .collect();
    let iris = images.add(sprite_image(Rgba { width: SCREEN_W, height: SCREEN_H, pixels: vec![0; (SCREEN_W * SCREEN_H * 4) as usize] }));
    let sky = load_one(&sma4_assets::sky_path(&dir.0, false), &mut images)
        .zip(load_one(&sma4_assets::sky_path(&dir.0, true), &mut images));
    commands.insert_resource(Art { message_box, text, iris, sky, ..default() });
}

/// Camera-local transform of a `w` x `h` px piece whose top-left is at GBA screen pixel
/// (x, y).
fn screen_px(x: i32, y: i32, w: u32, h: u32, z: f32) -> Transform {
    Transform::from_xyz(
        x as f32 + w as f32 / 2.0 - SCREEN_W as f32 / 2.0,
        SCREEN_H as f32 / 2.0 - y as f32 - h as f32 / 2.0,
        z,
    )
}

fn sync_objs(
    mut commands: Commands,
    game: Res<GameRes>,
    level: Res<LevelRes>,
    dir: Res<crate::AssetDir>,
    mut art: ResMut<Art>,
    mut images: ResMut<Assets<Image>>,
    old: Query<Entity, (With<CastleSprite>, Without<BoxPiece>)>,
    camera: Single<(Entity, &Transform), With<Camera2d>>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    let g = &game.0;
    if g.screen != sma4_core::Screen::Level || g.obj_sprites.is_empty() {
        return;
    }
    let (camera, cam) = *camera;
    // OAM positions are relative to the core's camera; the view follows the player and
    // stops at the level's edges (`player::follow_camera`), so it can sit elsewhere (a
    // window of another shape, a debug start). The curtain's sprites belong to the screen.
    let (gx, gy) = g.camera();
    let shift = if g.curtain.is_some() {
        Vec2::ZERO
    } else {
        Vec2::new(
            (gx + SCREEN_W as i32 / 2) as f32 - cam.translation.x,
            -((gy + SCREEN_H as i32 / 2) as f32) - cam.translation.y,
        )
    };
    let room = level.0.level_id;
    let art = &mut *art;
    let bank = art.banks.entry(room).or_insert_with(|| match ObjBank::read(&dir.0, room) {
        Ok(mut b) => {
            // Larry's flash colours as palette 16 (`sma4_core::larry::FLASH_PALETTE`).
            if room == sma4_core::wand::CABIN {
                match std::fs::read(sma4_assets::larry_flash_path(&dir.0)) {
                    Ok(p) => b.palette.extend_from_slice(&p),
                    Err(e) => warn!("{e}; re-run sma4-extract for Larry's flash palette"),
                }
            }
            Some(b)
        }
        Err(e) => {
            warn!("{e}; re-run sma4-extract for the King's castle");
            None
        }
    });
    let Some(bank) = bank else { return };
    let mut pieces = Vec::new();
    // Lower OAM entries are drawn on top.
    for (i, s) in g.obj_sprites.iter().enumerate() {
        let (w, h) = s.dims();
        // One picture per tile, size, palette and flips (not per position).
        let key = ObjSprite { x: 0, y: 0, ..*s };
        let image = art
            .cache
            .entry((room, key))
            .or_insert_with(|| {
                let px = bank.render(key.tile, (w, h), key.pal, key.hflip, key.vflip);
                images.add(sprite_image(Rgba { width: w, height: h, pixels: px }))
            })
            .clone();
        let z = if g.curtain.is_some() {
            Z_CURTAIN_OBJ
        } else if s.prio == 3 {
            Z_OBJ_BEHIND
        } else if i < g.obj_front {
            Z_OBJ_FRONT
        } else {
            Z_OBJ
        } - i as f32 * 0.001;
        let mut sprite = Sprite::from_image(image);
        if s.blend {
            sprite.color = Color::srgba(1.0, 1.0, 1.0, 0.5);
        }
        let mut t = screen_px(s.x, s.y, w, h, z);
        t.translation += shift.extend(0.0);
        pieces.push((sprite, t));
    }
    commands.entity(camera).with_children(|cam| {
        for (sprite, t) in pieces {
            cam.spawn((CastleSprite, sprite, t));
        }
    });
}

#[derive(Component)]
struct BoxPiece;

/// A piece of a box image: (x, y, w, h) px within it, drawn where it sits in the box.
fn box_piece(image: &Handle<Image>, (x, y, w, h): (u32, u32, u32, u32), z: f32) -> (Sprite, Transform) {
    let mut sprite = Sprite::from_image(image.clone());
    sprite.rect = Some(Rect::new(x as f32, y as f32, (x + w) as f32, (y + h) as f32));
    (sprite, screen_px((CASTLE_BOX_AT.0 * 8 + x) as i32, (CASTLE_BOX_AT.1 * 8 + y) as i32, w, h, z))
}

fn sync_box(
    mut commands: Commands,
    game: Res<GameRes>,
    art: Res<Art>,
    old: Query<Entity, With<BoxPiece>>,
    camera: Single<Entity, With<Camera2d>>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    let g = &game.0;
    if g.screen != sma4_core::Screen::Level {
        return;
    }
    let Some(t) = &g.throne else { return };
    let mut pieces = Vec::new();
    let rows = t.box_rows() as u32;
    if rows > 0
        && let Some(image) = &art.message_box
    {
        pieces.push(box_piece(image, (0, 0, CASTLE_BOX_W * 8, rows.min(CASTLE_BOX_H) * 8), Z_BOX));
    }
    if let Some((_, image)) = art.text.iter().find(|(id, _)| *id == t.message_id()) {
        for (col, row) in t.text_cells() {
            let (tx, ty) = (CASTLE_TEXT_AT.0 + col, CASTLE_TEXT_AT.1 + row);
            pieces.push(box_piece(image, (tx * 8, ty * 8, 8, 8), Z_BOX + 0.5));
        }
    }
    commands.entity(*camera).with_children(|cam| {
        for (sprite, t) in pieces {
            cam.spawn((BoxPiece, CastleSprite, sprite, t));
        }
    });
}

#[derive(Component)]
struct CurtainPiece;

/// The curtain: black (or a fade to it) over the whole view, or black outside the iris.
fn sync_curtain(
    mut commands: Commands,
    game: Res<GameRes>,
    mut art: ResMut<Art>,
    mut images: ResMut<Assets<Image>>,
    old: Query<Entity, With<CurtainPiece>>,
    camera: Single<Entity, With<Camera2d>>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    let Some(c) = &game.0.curtain else { return };
    let big = 4096.0;
    let mut pieces = Vec::new();
    if let Some(f) = c.fade() {
        let sprite = Sprite { color: Color::srgba(0.0, 0.0, 0.0, f as f32 / 31.0), custom_size: Some(Vec2::splat(big)), ..default() };
        pieces.push((sprite, Transform::from_xyz(0.0, 0.0, Z_CURTAIN)));
    } else if let Some(r) = c.iris() {
        if art.iris_drawn != Some(r) {
            art.iris_drawn = Some(r);
            let handle = art.iris.clone();
            if let Some(mut image) = images.get_mut(&handle) {
                crate::spade::draw_iris(&mut image, sma4_core::spade::IRIS_CENTRE, r);
            }
        }
        pieces.push((Sprite::from_image(art.iris.clone()), Transform::from_xyz(0.0, 0.0, Z_CURTAIN)));
        // Black beyond the GBA screen's four edges.
        let (w, h) = (SCREEN_W as f32, SCREEN_H as f32);
        for (x, y, bw, bh) in [(-big, -big, big, 3.0 * big), (w, -big, big, 3.0 * big), (0.0, -big, w, big), (0.0, h, w, big)] {
            let sprite = Sprite { color: Color::BLACK, custom_size: Some(Vec2::new(bw, bh)), ..default() };
            let t = Transform::from_xyz(x + bw / 2.0 - w / 2.0, h / 2.0 - y - bh / 2.0, Z_CURTAIN);
            pieces.push((sprite, t));
        }
    }
    commands.entity(*camera).with_children(|cam| {
        for (sprite, t) in pieces {
            cam.spawn((CurtainPiece, sprite, t));
        }
    });
}

#[derive(Component)]
struct SkyPiece;

/// The airship's room.
const AIRSHIP: u16 = 0x205;
/// BG2 behind everything; BG3 (priority 0) over the level and the sprites.
const Z_SKY_BACK: f32 = -10.0;
const Z_SKY_FRONT: f32 = 20.0;

/// The airship's two cloud layers (`sma4_assets::sky_path`), repeated to cover the view.
fn sync_sky(
    mut commands: Commands,
    game: Res<GameRes>,
    level: Res<LevelRes>,
    art: Res<Art>,
    old: Query<Entity, With<SkyPiece>>,
    camera: Single<Entity, With<Camera2d>>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    let g = &game.0;
    if g.screen != sma4_core::Screen::Level || level.0.level_id != AIRSHIP {
        return;
    }
    let Some((back, front)) = &art.sky else { return };
    let (cx, _) = g.camera();
    let (w, h) = (512, 256);
    let mut pieces = Vec::new();
    for (image, hofs, vofs, z) in [
        (back, cx >> 1, sma4_assets::SKY_BACK_Y, Z_SKY_BACK),
        (front, g.frame as i32, sma4_assets::SKY_FRONT_Y, Z_SKY_FRONT),
    ] {
        // The layer's pixel at the screen's top-left is (hofs, vofs), modulo its size.
        let (x0, y0) = (-(hofs.rem_euclid(w)) - w, -(vofs.rem_euclid(h)) - h);
        for i in 0..4 {
            for j in 0..4 {
                pieces.push((Sprite::from_image(image.clone()), screen_px(x0 + i * w, y0 + j * h, w as u32, h as u32, z)));
            }
        }
    }
    commands.entity(*camera).with_children(|cam| {
        for (sprite, t) in pieces {
            cam.spawn((SkyPiece, sprite, t));
        }
    });
}

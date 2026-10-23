//! Toad Houses: Toad, his message box (BG3, fixed on the screen) and the chest's item, all
//! extracted from the user's own ROM by `sma4-extract` (`objects/toad.png`,
//! `toad_box.png`, `toad_text.png`, `warp_whistle.png`). The house itself runs in
//! `sma4-core` (`toad_house.rs`).

use bevy::prelude::*;
use sma4_assets::{
    KOOPA_CELL_H, KOOPA_CELL_W, KOOPA_ORIGIN, Rgba, TOAD_BOX_AT, TOAD_RAISE, TOAD_BOX_H, TOAD_BOX_W, TOAD_TEXT_AT,
    chest_lid_path, house_item_path, toad_box_path, toad_path, toad_text_path,
};
use sma4_core::{SCREEN_H, SCREEN_W};

use crate::GameRes;
use crate::player::{load_cells, sprite_image};

pub struct ToadHousePlugin;

impl Plugin for ToadHousePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_art).add_systems(Update, sync_house);
    }
}

/// The open lid (BG tiles) just over the level, Toad with the enemies, the item over the
/// tiles.
const Z_LID: f32 = 0.5;
const Z_TOAD: f32 = 7.0;
const Z_ITEM: f32 = 8.0;
/// BG3 has priority 0 (Toad's init): over the level and the sprites, under the status bar
/// (z 50).
const Z_BOX: f32 = 30.0;

#[derive(Resource)]
struct Art {
    toad: Vec<Handle<Image>>,
    message_box: Option<Handle<Image>>,
    text: Option<Handle<Image>>,
    item: Option<Handle<Image>>,
    lid: Option<Handle<Image>>,
}

#[derive(Component)]
struct HouseSprite;

fn load_one(path: &std::path::Path, images: &mut Assets<Image>) -> Option<Handle<Image>> {
    match Rgba::read_png(path) {
        Ok(img) => Some(images.add(sprite_image(img))),
        Err(e) => {
            warn!("{e}; re-run sma4-extract");
            None
        }
    }
}

fn load_art(mut commands: Commands, dir: Res<crate::AssetDir>, mut images: ResMut<Assets<Image>>) {
    let art = Art {
        toad: load_cells(&toad_path(&dir.0), KOOPA_CELL_W, KOOPA_CELL_H, &mut images),
        message_box: load_one(&toad_box_path(&dir.0), &mut images),
        text: load_one(&toad_text_path(&dir.0), &mut images),
        item: load_one(&house_item_path(&dir.0), &mut images),
        lid: load_one(&chest_lid_path(&dir.0), &mut images),
    };
    commands.insert_resource(art);
}

/// Camera-local transform of a `w` x `h` px piece whose top-left is at GBA screen pixel
/// (x, y).
fn screen_px(x: u32, y: u32, w: u32, h: u32) -> Transform {
    Transform::from_xyz(
        x as f32 + w as f32 / 2.0 - SCREEN_W as f32 / 2.0,
        SCREEN_H as f32 / 2.0 - y as f32 - h as f32 / 2.0,
        Z_BOX,
    )
}

/// A piece of a box image: (x, y, w, h) px within it, drawn where it sits in the box.
fn box_piece(image: &Handle<Image>, (x, y, w, h): (u32, u32, u32, u32)) -> (Sprite, Transform) {
    let mut sprite = Sprite::from_image(image.clone());
    sprite.rect = Some(Rect::new(x as f32, y as f32, (x + w) as f32, (y + h) as f32));
    (sprite, screen_px(TOAD_BOX_AT.0 * 8 + x, TOAD_BOX_AT.1 * 8 + y, w, h))
}

fn sync_house(
    mut commands: Commands,
    game: Res<GameRes>,
    art: Res<Art>,
    old: Query<Entity, With<HouseSprite>>,
    camera: Single<Entity, With<Camera2d>>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    let g = &game.0;
    if g.screen != sma4_core::Screen::Level {
        return;
    }
    let Some(house) = &g.house else { return };
    if let Some(image) = art.toad.get(house.toad_right as usize) {
        let (x, y) = house.toad;
        let cx = x - KOOPA_ORIGIN.0 + KOOPA_CELL_W as i32 / 2;
        let cy = y - (KOOPA_ORIGIN.1 - TOAD_RAISE) + KOOPA_CELL_H as i32 / 2;
        commands.spawn((HouseSprite, Sprite::from_image(image.clone()), Transform::from_xyz(cx as f32, -(cy as f32), Z_TOAD)));
    }
    if let (Some((x, y)), Some(image)) = (house.chest_lid, &art.lid) {
        let t = Transform::from_xyz((x + 8) as f32, -((y + 8) as f32), Z_LID);
        commands.spawn((HouseSprite, Sprite::from_image(image.clone()), t));
    }
    if let Some(item) = &house.item
        && item.visible
        && let Some(image) = &art.item
    {
        // OAM y = y - 1 (0x0804FFFC).
        let (x, y) = item.px();
        let t = Transform::from_xyz((x + 8) as f32, -((y - 1 + 8) as f32), Z_ITEM);
        commands.spawn((HouseSprite, Sprite::from_image(image.clone()), t));
    }
    let mut pieces = Vec::new();
    let rows = house.box_rows() as u32;
    if rows > 0
        && let Some(image) = &art.message_box
    {
        pieces.push(box_piece(image, (0, 0, TOAD_BOX_W * 8, rows.min(TOAD_BOX_H) * 8)));
    }
    if let Some(image) = &art.text {
        for (col, row) in house.text_cells() {
            let (tx, ty) = (TOAD_TEXT_AT.0 + col, TOAD_TEXT_AT.1 + row);
            let mut piece = box_piece(image, (tx * 8, ty * 8, 8, 8));
            piece.1.translation.z += 0.5;
            pieces.push(piece);
        }
    }
    commands.entity(*camera).with_children(|cam| {
        for (sprite, t) in pieces {
            cam.spawn((HouseSprite, sprite, t));
        }
    });
}

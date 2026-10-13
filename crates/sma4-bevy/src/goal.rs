//! End of the level: the goal roulette card and its box, the coins the enemies turn into,
//! and the course-clear texts, all extracted from the user's own ROM by `sma4-extract`
//! (`objects/goal.png`, `coin_spin.png`, `course_clear.png`, `card_got.png`,
//! the card in the frame drawn with the card sprite). The sequence itself runs in `sma4-core` (`goal.rs`).

use bevy::prelude::*;
use sma4_assets::{
    CARD_GOT_AT, CARD_GOT_TILES, CLEAR_CARD_AT, COURSE_CLEAR_AT, COURSE_CLEAR_TILES, GOAL_BOX, GOAL_FLYING,
    KOOPA_CELL_H, KOOPA_CELL_W, KOOPA_ORIGIN, Rgba, card_got_path, coin_spin_path,
    course_clear_path, goal_path,
};
use sma4_core::{SCREEN_H, SCREEN_W, goal::Phase};

use crate::GameRes;
use crate::player::{load_cells, sprite_image};

pub struct GoalPlugin;

impl Plugin for GoalPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_art).add_systems(Update, sync_goal);
    }
}

/// Card and coins with the enemies; the box behind the card.
const Z_BOX: f32 = 7.0;
const Z_CARD: f32 = 8.0;
/// Texts over the level, under the status bar (z 50).
const Z_TEXT: f32 = 30.0;

#[derive(Resource)]
struct Art {
    goal: Vec<Handle<Image>>,
    coins: Vec<Handle<Image>>,
    course_clear: Option<Handle<Image>>,
    card_got: Option<Handle<Image>>,
}

#[derive(Component)]
struct GoalSprite;

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
        goal: load_cells(&goal_path(&dir.0), KOOPA_CELL_W, KOOPA_CELL_H, &mut images),
        coins: load_cells(&coin_spin_path(&dir.0), 16, 16, &mut images),
        course_clear: load_one(&course_clear_path(&dir.0), &mut images),
        card_got: load_one(&card_got_path(&dir.0), &mut images),
    };
    commands.insert_resource(art);
}

/// Level-space transform of a goal cell whose object position is (x, y).
fn cell_at(x: i32, y: i32, z: f32) -> Transform {
    let cx = x - KOOPA_ORIGIN.0 + KOOPA_CELL_W as i32 / 2;
    let cy = y - KOOPA_ORIGIN.1 + KOOPA_CELL_H as i32 / 2;
    Transform::from_xyz(cx as f32, -(cy as f32), z)
}

/// Camera-local transform of an image `w` x `h` px whose top-left is at GBA screen tile
/// (col, row).
fn screen_at((col, row): (u32, u32), w: u32, h: u32) -> Transform {
    let (x, y) = ((col * 8) as f32, (row * 8) as f32);
    Transform::from_xyz(
        x + w as f32 / 2.0 - SCREEN_W as f32 / 2.0,
        SCREEN_H as f32 / 2.0 - y - h as f32 / 2.0,
        Z_TEXT,
    )
}

fn sync_goal(
    mut commands: Commands,
    game: Res<GameRes>,
    art: Res<Art>,
    old: Query<Entity, With<GoalSprite>>,
    camera: Single<Entity, With<Camera2d>>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    let g = &game.0;
    if g.screen != sma4_core::Screen::Level {
        return;
    }
    for c in &g.end_coins {
        // 0x0808EFD4 draws the coin like the block coin (`coin_spin.png` cells start at x).
        if let Some(image) = art.coins.get(c.spin_frame()) {
            let (x, y) = c.px();
            let t = Transform::from_xyz((x + 8) as f32, -((y + 8) as f32), Z_CARD);
            commands.spawn((GoalSprite, Sprite::from_image(image.clone()), t));
        }
    }
    let Some(goal) = &g.goal else { return };
    // The box stays where the card started; it fades out (blend b / 16) after the touch.
    if goal.box_fade != 0
        && let Some(image) = art.goal.get(GOAL_BOX)
    {
        let mut sprite = Sprite::from_image(image.clone());
        if goal.box_fade != 0xFF {
            sprite.color = Color::srgba(1.0, 1.0, 1.0, goal.box_fade as f32 / 16.0);
        }
        commands.spawn((GoalSprite, sprite, cell_at(goal.home.0, goal.home.1, Z_BOX)));
    }
    let cell = if goal.phase == Phase::Roulette { goal.card as usize } else { GOAL_FLYING + goal.card as usize };
    if matches!(goal.phase, Phase::Roulette | Phase::Rising)
        && let Some(image) = art.goal.get(cell)
    {
        let (x, y) = goal.px();
        commands.spawn((GoalSprite, Sprite::from_image(image.clone()), cell_at(x, y, Z_CARD)));
    }
    let (clear, card_line, card) = goal.texts();
    let mut texts = Vec::new();
    if clear && let Some(image) = &art.course_clear {
        let (w, h) = (COURSE_CLEAR_TILES.0 * 8, COURSE_CLEAR_TILES.1 * 8);
        texts.push((image.clone(), screen_at(COURSE_CLEAR_AT, w, h)));
    }
    if card_line && let Some(image) = &art.card_got {
        let (w, h) = (CARD_GOT_TILES.0 * 8, CARD_GOT_TILES.1 * 8);
        texts.push((image.clone(), screen_at(CARD_GOT_AT, w, h)));
    }
    if card && let Some(image) = art.goal.get(goal.card as usize) {
        // The ROM puts 2x2 BG tiles of the card there (0x080671AC); the card sprite cell,
        // with the object's position on the frame's inner cell, shows the same picture.
        let (col, row) = CLEAR_CARD_AT;
        let at = (col as i32 * 8 - KOOPA_ORIGIN.0, row as i32 * 8 - KOOPA_ORIGIN.1);
        let mut t = screen_at((0, 0), KOOPA_CELL_W, KOOPA_CELL_H);
        t.translation.x += at.0 as f32;
        t.translation.y -= at.1 as f32;
        t.translation.z += 1.0;
        texts.push((image.clone(), t));
    }
    commands.entity(*camera).with_children(|cam| {
        for (image, t) in texts {
            cam.spawn((GoalSprite, Sprite::from_image(image), t));
        }
    });
}

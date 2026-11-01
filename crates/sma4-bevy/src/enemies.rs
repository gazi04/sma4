//! Enemy sprites, extracted from the user's own ROM by `sma4-extract`: the Goomba
//! (`objects/walker.png`), the Paragoombas (`objects/paragoomba_*.png`), the Micro-Goombas
//! (`objects/micro_goomba.png`), the Koopa Troopas (`objects/koopa_*.png`), the Piranha Plants
//! (`objects/piranha_*.png`, the Venus Fire Trap included), its fireballs
//! (`objects/fireball.png`) and the puff of smoke (`objects/puff.png`). Behaviour and the
//! frame to show come from `sma4-core`.

use std::collections::HashMap;

use bevy::prelude::*;
use sma4_assets::{
    FIREBALL_FRAMES, KOOPA_CELL_H, KOOPA_CELL_W, KOOPA_ORIGIN, PARAGOOMBA_GOOMBA, PARAGOOMBA_SQUISHED, PIRANHA_ORIGIN,
    WALKER_SQUISHED, fireball_path, koopa_path, micro_goomba_path, paragoomba_path, piranha_path, puff_path,
    walker_path,
};
use sma4_core::{EnemyState, enemies};

use crate::GameRes;
use crate::player::load_cells;

pub struct EnemyPlugin;

impl Plugin for EnemyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_art).add_systems(Update, sync_enemies);
    }
}

const Z_ENEMY: f32 = 8.0;
/// A Goomba coming out of a pipe is drawn behind the scenery (+0x1A bit 4, 0x08053664),
/// over the background scene.
const Z_BEHIND: f32 = -1.0;
/// Riding Micro-Goombas show over the player (0x0808D7E0 uses OBJ priority 1, like it).
const Z_MICRO: f32 = 10.5;

/// Sprite cells per enemy type, the puff of smoke, the fireball, the Micro-Goomba, the
/// boomerang and the hammer.
#[derive(Resource)]
struct Art(
    HashMap<u16, Vec<Handle<Image>>>,
    Vec<Handle<Image>>,
    Vec<Handle<Image>>,
    Vec<Handle<Image>>,
    Vec<Handle<Image>>,
    Vec<Handle<Image>>,
);

#[derive(Component)]
struct EnemySprite;

fn load_art(mut commands: Commands, dir: Res<crate::AssetDir>, mut images: ResMut<Assets<Image>>) {
    let mut art = HashMap::new();
    art.insert(enemies::WALKER, load_cells(&walker_path(&dir.0), 16, 16, &mut images));
    for kind in [enemies::KOOPA, enemies::RED_KOOPA, enemies::PARATROOPA, enemies::RED_PARATROOPA, enemies::BUZZY] {
        art.insert(kind, load_cells(&koopa_path(&dir.0, kind), KOOPA_CELL_W, KOOPA_CELL_H, &mut images));
    }
    for kind in [enemies::PARAGOOMBA, enemies::MICRO_PARAGOOMBA] {
        art.insert(kind, load_cells(&paragoomba_path(&dir.0, kind), KOOPA_CELL_W, KOOPA_CELL_H, &mut images));
    }
    for kind in enemies::PIRANHA_TYPES {
        art.insert(kind, load_cells(&piranha_path(&dir.0, kind), KOOPA_CELL_W, KOOPA_CELL_H, &mut images));
    }
    // World 1-4's lift: one cell.
    let (lw, lh) = sma4_assets::LIFT_CELL;
    art.insert(sma4_core::lifts::LIFT, load_cells(&sma4_assets::lift_path(&dir.0), lw, lh, &mut images));
    let puff = load_cells(&puff_path(&dir.0), 16, 16, &mut images);
    let fireball = load_cells(&fireball_path(&dir.0), KOOPA_CELL_W, KOOPA_CELL_H, &mut images);
    if fireball.is_empty() {
        warn!("no objects/fireball.png; re-run sma4-extract");
    }
    let micro = load_cells(&micro_goomba_path(&dir.0), KOOPA_CELL_W, KOOPA_CELL_H, &mut images);
    if micro.is_empty() {
        warn!("no objects/micro_goomba.png; re-run sma4-extract");
    }
    art.insert(
        enemies::BOOMERANG_BRO,
        load_cells(&sma4_assets::boomerang_bro_path(&dir.0), KOOPA_CELL_W, KOOPA_CELL_H, &mut images),
    );
    let boomerang = load_cells(&sma4_assets::boomerang_path(&dir.0), KOOPA_CELL_W, KOOPA_CELL_H, &mut images);
    // The map's Hammer Bro battles.
    art.insert(enemies::HAMMER_BRO, load_cells(&sma4_assets::hammer_bro_path(&dir.0), KOOPA_CELL_W, KOOPA_CELL_H, &mut images));
    let hammer = load_cells(&sma4_assets::hammer_path(&dir.0), KOOPA_CELL_W, KOOPA_CELL_H, &mut images);
    if hammer.is_empty() {
        warn!("no objects/hammer.png; re-run sma4-extract");
    }
    commands.insert_resource(Art(art, puff, fireball, micro, boomerang, hammer));
}

fn sync_enemies(mut commands: Commands, game: Res<GameRes>, art: Res<Art>, old: Query<Entity, With<EnemySprite>>) {
    for e in &old {
        commands.entity(e).despawn();
    }
    // Fireballs: cells 0-3 fly right, 4-7 left; the slot position sits at KOOPA_ORIGIN.
    for f in &game.0.fireballs {
        let cell = f.frame() + if f.vx < 0 { FIREBALL_FRAMES as usize } else { 0 };
        if let Some(image) = art.2.get(cell) {
            let (x, y) = f.px();
            let (w, h) = (KOOPA_CELL_W as i32, KOOPA_CELL_H as i32);
            let (cx, cy) = (x - KOOPA_ORIGIN.0 + w / 2, y - KOOPA_ORIGIN.1 + h / 2);
            commands.spawn((EnemySprite, Sprite::from_image(image.clone()), Transform::from_xyz(cx as f32, -(cy as f32), Z_ENEMY + 0.5)));
        }
    }
    // Fire Mario's fireballs: spin frame (frame counter >> 2) & 3, the "flying left" cells
    // when they go left; bursting, the puff's 16x16 cells 4 px left of the slot.
    for f in &game.0.player_fireballs {
        let (x, y) = f.px();
        if !f.live() {
            if let Some(image) = art.1.get(f.puff_cell()) {
                let t = Transform::from_xyz((x - 4 + 8) as f32, -((y + 8) as f32), Z_ENEMY + 0.5);
                commands.spawn((EnemySprite, Sprite::from_image(image.clone()), t));
            }
            continue;
        }
        let cell = f.frame(game.0.frame) + if f.vx < 0 { FIREBALL_FRAMES as usize } else { 0 };
        if let Some(image) = art.2.get(cell) {
            let (w, h) = (KOOPA_CELL_W as i32, KOOPA_CELL_H as i32);
            let (cx, cy) = (x - KOOPA_ORIGIN.0 + w / 2, y - KOOPA_ORIGIN.1 + h / 2);
            commands.spawn((EnemySprite, Sprite::from_image(image.clone()), Transform::from_xyz(cx as f32, -(cy as f32), Z_ENEMY + 0.5)));
        }
    }
    // Boomerangs: the slot position at KOOPA_ORIGIN.
    for b in &game.0.boomerangs {
        if let Some(image) = art.4.get(b.cell()) {
            let (x, y) = b.px();
            let (w, h) = (KOOPA_CELL_W as i32, KOOPA_CELL_H as i32);
            let (cx, cy) = (x - KOOPA_ORIGIN.0 + w / 2, y - KOOPA_ORIGIN.1 + h / 2);
            commands.spawn((EnemySprite, Sprite::from_image(image.clone()), Transform::from_xyz(cx as f32, -(cy as f32), Z_ENEMY + 0.5)));
        }
    }
    // Hammers: the slot position at KOOPA_ORIGIN.
    for h in &game.0.hammers {
        if let Some(image) = art.5.get(h.cell()) {
            let (x, y) = h.px();
            let (w, ch) = (KOOPA_CELL_W as i32, KOOPA_CELL_H as i32);
            let (cx, cy) = (x - KOOPA_ORIGIN.0 + w / 2, y - KOOPA_ORIGIN.1 + ch / 2);
            commands.spawn((EnemySprite, Sprite::from_image(image.clone()), Transform::from_xyz(cx as f32, -(cy as f32), Z_ENEMY + 0.5)));
        }
    }
    // Micro-Goombas: the slot position (top-left of the 8x8 OBJ) sits at KOOPA_ORIGIN; in
    // front of the player.
    for m in &game.0.micro_goombas {
        if let Some(image) = art.3.get(m.cell(game.0.frame)) {
            let (x, y) = m.px();
            let (w, h) = (KOOPA_CELL_W as i32, KOOPA_CELL_H as i32);
            let (cx, cy) = (x - KOOPA_ORIGIN.0 + w / 2, y - KOOPA_ORIGIN.1 + h / 2);
            commands.spawn((EnemySprite, Sprite::from_image(image.clone()), Transform::from_xyz(cx as f32, -(cy as f32), Z_MICRO)));
        }
    }
    // Lifts: the object's position at LIFT_ORIGIN in the cell.
    if let Some(image) = art.0.get(&sma4_core::lifts::LIFT).and_then(|c| c.first()) {
        let ((w, h), (ox, oy)) = (sma4_assets::LIFT_CELL, sma4_assets::LIFT_ORIGIN);
        for l in &game.0.lifts {
            let (x, y) = l.px();
            let (cx, cy) = (x - ox + w as i32 / 2, y - oy + h as i32 / 2);
            commands.spawn((EnemySprite, Sprite::from_image(image.clone()), Transform::from_xyz(cx as f32, -(cy as f32), Z_ENEMY)));
        }
    }
    for e in &game.0.enemies {
        let (x, y) = e.px();
        if let EnemyState::Puff(_) = e.state {
            // State 8 draws at the object's position (0x08044550).
            if let Some(image) = art.1.get(e.sprite_cell(game.0.frame)) {
                let t = Transform::from_xyz((x + 8) as f32, -((y + 8) as f32), Z_ENEMY);
                commands.spawn((EnemySprite, Sprite::from_image(image.clone()), t));
            }
            continue;
        }
        if e.is_plant() {
            if e.plant_visible()
                && let Some(image) = art.0.get(&e.kind).and_then(|cells| cells.get(e.sprite_cell(game.0.frame)))
            {
                spawn_plant(&mut commands, image, e);
            }
            continue;
        }
        // A stomped Paragoomba is a Goomba in the Paragoomba's palette: its sheet's last cells.
        let (kind, cell) = if e.was_para {
            let cell = e.sprite_cell(game.0.frame);
            let cell = if cell == WALKER_SQUISHED { PARAGOOMBA_SQUISHED } else { PARAGOOMBA_GOOMBA + cell };
            (enemies::PARAGOOMBA, cell)
        } else {
            (e.kind, e.sprite_cell(game.0.frame))
        };
        let Some(image) = art.0.get(&kind).and_then(|cells| cells.get(cell)) else {
            continue;
        };
        let dead = e.state == EnemyState::Dead || e.flipped;
        let mut sprite = Sprite::from_image(image.clone());
        sprite.flip_y = dead;
        // Cell centre in level pixels. Koopa cells put the 16x16 box at KOOPA_ORIGIN; flipped
        // upside down, the shell must stay over the box, so the centre moves down 16 px.
        let (cx, cy) = if kind == enemies::WALKER {
            (x + 8, y + 8)
        } else {
            // Koopa art faces left; walking right mirrors it (0x080539E8 shifts the head).
            sprite.flip_x = e.is_koopa() && e.state == EnemyState::Walking && e.vx > 0;
            let (w, h) = (KOOPA_CELL_W as i32, KOOPA_CELL_H as i32);
            // The Bro's cells sit BRO_RAISE px higher (its body hangs below its y).
            let raise = if enemies::is_bro(kind) { sma4_assets::BRO_RAISE } else { 0 };
            (x - KOOPA_ORIGIN.0 + w / 2, y - KOOPA_ORIGIN.1 + raise + h / 2 + if dead { 16 } else { 0 })
        };
        let z = if e.emerging > 0 { Z_BEHIND } else { Z_ENEMY };
        commands.spawn((EnemySprite, sprite, Transform::from_xyz(cx as f32, -(cy as f32), z)));
    }
}

/// A Piranha Plant cell, cut off at its pipe top: the ROM hides the part inside the pipe
/// behind two mask OBJs (0x080A39EC) covering the 32 px above `home_y`.
fn spawn_plant(commands: &mut Commands, image: &Handle<Image>, e: &enemies::Enemy) {
    let (x, y) = e.px();
    let top = y - PIRANHA_ORIGIN.1;
    // A floor plant shows above its pipe top (`home_y`), a ceiling plant below its pipe's
    // bottom edge (its home y too).
    let cut = (e.home_y - top).clamp(0, KOOPA_CELL_H as i32);
    let (from, to) = if e.is_ceiling_plant() { (cut, KOOPA_CELL_H as i32) } else { (0, cut) };
    if to <= from {
        return;
    }
    let mut sprite = Sprite::from_image(image.clone());
    sprite.rect = Some(Rect::new(0.0, from as f32, KOOPA_CELL_W as f32, to as f32));
    let cx = x - PIRANHA_ORIGIN.0 + KOOPA_CELL_W as i32 / 2;
    let cy = top as f32 + (from + to) as f32 / 2.0;
    commands.spawn((EnemySprite, sprite, Transform::from_xyz(cx as f32, -cy, Z_ENEMY)));
}

//! The mini-fortress's objects (`objects/fortress_*.png`, extracted from the user's own ROM):
//! Podoboos and their embers and splashes, Roto-Discs, Dry Bones, Boom Boom, his sparks and
//! ball, the puffs of smoke and the treasure chest; and the crushing ceiling's split screen
//! (the level's rows above the split drawn `Level::shift` px lower). Behaviour comes from
//! `sma4_core::fortress`, `sma4_core::boom_boom` and `sma4_core::crusher`.

use bevy::prelude::*;
use bevy::sprite_render::{TilemapChunk, TilemapChunkTileData};
use sma4_assets::{
    BALL_SHEET, BOOM_BOOM_SHEET, CHEST_SHEET, DRY_BONES_SHEET, EMBER_SHEET, FortressSheet, LAVA_SPLASH_SHEET, PODOBOO_SHEET,
    ROTO_DISC_SHEET, SMOKE_SHEET, SPARK_SHEET,
};
use sma4_core::{EnemyState, enemies};

use crate::GameRes;
use crate::level::LevelTiles;
use crate::player::load_cells;

pub struct FortressPlugin;

impl Plugin for FortressPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_art).add_systems(Update, (sync_fortress, sync_split));
    }
}

/// Over the level's tiles (z 0), under the player (10).
const Z_OBJECT: f32 = 8.0;
const Z_EFFECT: f32 = 9.0;
/// The Podoboos rise out of the lava: behind its tiles.
const Z_PODOBOO: f32 = -0.5;
/// The split's copies of the level: the moving part over the level, the fixed floor over it.
const Z_UPPER: f32 = 0.2;
const Z_LOWER: f32 = 0.3;

#[derive(Resource, Default)]
struct Art {
    podoboo: Vec<Handle<Image>>,
    ember: Vec<Handle<Image>>,
    splash: Vec<Handle<Image>>,
    disc: Vec<Handle<Image>>,
    dry_bones: Vec<Handle<Image>>,
    boom_boom: Vec<Handle<Image>>,
    ball: Vec<Handle<Image>>,
    spark: Vec<Handle<Image>>,
    smoke: Vec<Handle<Image>>,
    chest: Vec<Handle<Image>>,
    battle_chest: Vec<Handle<Image>>,
}

#[derive(Component)]
struct FortressSprite;

fn load_art(mut commands: Commands, dir: Res<crate::AssetDir>, mut images: ResMut<Assets<Image>>) {
    let mut load = |s: FortressSheet| load_cells(&s.path(&dir.0), s.cell.0, s.cell.1, &mut images);
    commands.insert_resource(Art {
        podoboo: load(PODOBOO_SHEET),
        ember: load(EMBER_SHEET),
        splash: load(LAVA_SPLASH_SHEET),
        disc: load(ROTO_DISC_SHEET),
        dry_bones: load(DRY_BONES_SHEET),
        boom_boom: load(BOOM_BOOM_SHEET),
        ball: load(BALL_SHEET),
        spark: load(SPARK_SHEET),
        smoke: load(SMOKE_SHEET),
        chest: load(CHEST_SHEET),
        battle_chest: load(sma4_assets::BATTLE_CHEST_SHEET),
    });
}

/// A sheet cell drawn with the object's position (px, level coordinates) at its origin.
fn spawn(commands: &mut Commands, sheet: FortressSheet, cells: &[Handle<Image>], cell: usize, (x, y): (i32, i32), z: f32) -> Option<()> {
    spawn_flipped(commands, sheet, cells, cell, (x, y), z, false)
}

fn spawn_flipped(
    commands: &mut Commands,
    sheet: FortressSheet,
    cells: &[Handle<Image>],
    cell: usize,
    (x, y): (i32, i32),
    z: f32,
    flip_y: bool,
) -> Option<()> {
    let image = cells.get(cell)?;
    let (w, h) = (sheet.cell.0 as i32, sheet.cell.1 as i32);
    let (cx, cy) = (x - sheet.origin.0 + w / 2, y - sheet.origin.1 + h / 2);
    let mut sprite = Sprite::from_image(image.clone());
    sprite.flip_y = flip_y;
    commands.spawn((FortressSprite, sprite, Transform::from_xyz(cx as f32, -(cy as f32), z)));
    Some(())
}

fn sync_fortress(mut commands: Commands, game: Res<GameRes>, art: Res<Art>, old: Query<Entity, With<FortressSprite>>) {
    for e in &old {
        commands.entity(e).despawn();
    }
    let g = &game.0;
    let c = &mut commands;
    for p in g.podoboos.iter().filter(|p| !p.hidden()) {
        let cell = p.frame as usize + if p.falling() { 3 } else { 0 };
        spawn_flipped(c, PODOBOO_SHEET, &art.podoboo, cell, p.px(), Z_PODOBOO, p.dead);
    }
    for e in &g.embers {
        spawn(c, EMBER_SHEET, &art.ember, e.step as usize, (e.x, e.y), Z_EFFECT);
    }
    for s in &g.lava_splashes {
        spawn(c, LAVA_SPLASH_SHEET, &art.splash, s.index(), (s.x, s.y), Z_EFFECT);
    }
    for d in &g.discs {
        spawn(c, ROTO_DISC_SHEET, &art.disc, (g.frame & 15) as usize, d.px(), Z_OBJECT);
    }
    // Dry Bones: frame +0x17, the right-facing cells after the left-facing ones.
    for e in g.enemies.iter().filter(|e| e.kind == enemies::DRY_BONES) {
        let cell = e.sprite_cell(g.frame) + if e.facing_right { 4 } else { 0 };
        spawn_flipped(c, DRY_BONES_SHEET, &art.dry_bones, cell, e.px(), Z_OBJECT, e.state == EnemyState::Dead);
    }
    if let Some(b) = &g.boom_boom {
        let cell = b.frame as usize + if b.facing_right { 12 } else { 0 };
        // Hit by a fireball (+0x25) he blinks.
        if b.flash & 2 == 0 {
            spawn(c, BOOM_BOOM_SHEET, &art.boom_boom, cell, b.px(), Z_OBJECT);
        }
    }
    if let Some(b) = &g.ball {
        spawn(c, BALL_SHEET, &art.ball, 0, b.px(), Z_OBJECT);
    }
    for (i, s) in g.sparks.iter().enumerate() {
        // Palette ((0x03003CF6 >> 2) + slot) & 3.
        spawn(c, SPARK_SHEET, &art.spark, ((g.frame >> 2) as usize + i) & 3, s.px(), Z_EFFECT);
    }
    for p in &g.puffs {
        spawn(c, SMOKE_SHEET, &art.smoke, (p.life >> 3) as usize, (p.x, p.y), Z_EFFECT);
    }
    if let Some(ch) = &g.chest {
        // A map object's battle has its own chest sheet (its item: a Starman in World 1).
        let (sheet, cells) =
            if g.battle.is_some() { (sma4_assets::BATTLE_CHEST_SHEET, &art.battle_chest) } else { (CHEST_SHEET, &art.chest) };
        if ch.item.is_none() && ch.visible() {
            spawn(c, sheet, cells, 0, ch.px(), Z_OBJECT);
        }
        if ch.item_shown().is_some() {
            spawn(c, sheet, cells, 1, ch.px(), Z_EFFECT);
        }
    }
}

/// The crushing ceiling's copies of the level: rows above the split, moved down by the
/// shift, and the floor rows below it; the level's own tilemaps are hidden meanwhile.
#[derive(Component)]
struct SplitTiles {
    upper: bool,
}

fn sync_split(
    mut commands: Commands,
    game: Res<GameRes>,
    mut maps: Query<(&LevelTiles, &TilemapChunk, &TilemapChunkTileData, &Transform, &mut Visibility), Without<SplitTiles>>,
    mut copies: Query<(Entity, &SplitTiles, &mut Transform), Without<LevelTiles>>,
) {
    let level = &game.0.level;
    let Some((split, d)) = level.shift else {
        for (e, ..) in &copies {
            commands.entity(e).despawn();
        }
        return;
    };
    if copies.is_empty() {
        let (w, h) = (level.width, level.height);
        let split_row = (split / sma4_core::level::TILE_PX) as usize;
        for (tiles, chunk, data, t, mut vis) in &mut maps {
            *vis = Visibility::Hidden;
            if tiles.0 != 0 || data.0.len() != w * h {
                continue;
            }
            for upper in [true, false] {
                // Rows are stored bottom first.
                let rows = data
                    .0
                    .iter()
                    .enumerate()
                    .map(|(i, &tile)| if ((h - 1 - i / w) < split_row) == upper { tile } else { None })
                    .collect();
                let z = if upper { Z_UPPER } else { Z_LOWER };
                commands.spawn((
                    SplitTiles { upper },
                    TilemapChunk { chunk_size: chunk.chunk_size, tile_display_size: chunk.tile_display_size, tileset: chunk.tileset.clone(), ..default() },
                    TilemapChunkTileData(rows),
                    Transform::from_xyz(t.translation.x, t.translation.y, z),
                ));
            }
        }
        return;
    }
    let base_y = maps.iter().find(|m| m.0.0 == 0).map(|m| m.3.translation.y);
    for (_, s, mut t) in &mut copies {
        if let Some(y) = base_y {
            t.translation.y = if s.upper { y - d as f32 } else { y };
        }
    }
}

//! The spade panel's bonus game: Toad's room and the picture rows, drawn over the GBA
//! screen from `spade/` (extracted from the user's own ROM by `sma4-extract`). The game
//! itself runs in `sma4_core::spade`.
//!
//! Layers as the ROM sets them: the room is BG2 under BG1 (`host.png`), Toad and Mario
//! (OBJ), then the message box on BG3. The slots are BG2's icons (redrawn from the map
//! the core keeps, `slot_tiles.png`), BG1's pictures (`slot_reels_<count>.png`) whose x the
//! HBlank handler 0x08033748 sets line by line from each row's position, the prize sprite
//! (OBJ priority 1) and the frame on BG3 (priority 0). The status bar is the map's.

use std::collections::HashMap;

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use sma4_assets::{
    PLAYER_H, PLAYER_W, Rgba, SPADE_BOX_AT, SPADE_BOX_ROWS, SPADE_BOX_W, SPADE_COUNTS, SPADE_FRAME, SPADE_HOST,
    SPADE_HOST_BOX, SPADE_HOST_TOAD, SPADE_MESSAGES, SPADE_TEXT_AT, SPADE_TILES, SpadeAsset, spade_host_text,
    spade_path, spade_prize, spade_reels,
};
use sma4_core::spade::{HOST_MARIO_OAM, Phase, Shade, Spade};
use sma4_core::{SCREEN_H, SCREEN_W, Screen};

use crate::GameRes;
use crate::player::{load_cells, sprite_image};

pub struct SpadePlugin;

impl Plugin for SpadePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, load_art).add_systems(Update, (debug_start, sync_spade).chain());
    }
}

/// Debug: start in the spade game (`--spade`), with this streak count.
#[derive(Resource, Default)]
pub struct DebugSpade(pub Option<u8>);

/// Over the off-level cover (z 40), under the map's status bar (43); the fades cover the
/// bar too (BLDCNT 0xFF, and 0x0800373C fades every palette).
const Z_BACK: f32 = 40.5;
const Z_PICTURES: f32 = 40.6;
const Z_SPRITES: f32 = 40.7;
const Z_BOX: f32 = 40.8;
const Z_TEXT: f32 = 40.9;
const Z_SHADE: f32 = 45.0;

/// Screen lines of the three rows: line L shows the x the HBlank of line L - 1 wrote
/// (0x0813181E: rows end at table lines 0x30 and 0x64). Line 0 is under the frame.
const ROW_LINES: [(u32, u32); 3] = [(1, 0x31), (0x31, 0x65), (0x65, SCREEN_H)];
/// BG1's vertical scroll (0x03003754 = -4).
const REELS_VOFS: i32 = -4;
/// The prize sprite's x (0x0803446C).
const PRIZE_X: i32 = 0x78;

#[derive(Resource)]
struct Art {
    asset: SpadeAsset,
    message_box: Handle<Image>,
    texts: Vec<Handle<Image>>,
    reels: Vec<Handle<Image>>,
    /// BG2 tile atlas (CPU side) and its index by map entry.
    tiles: Rgba,
    tile_index: HashMap<u16, usize>,
    /// BG2 as drawn last, and what it was drawn from.
    icons: Handle<Image>,
    drawn: Option<(u8, u32)>,
    prizes: Vec<(u8, Handle<Image>, (i32, i32))>,
    /// Mario standing, by form.
    mario: Vec<Handle<Image>>,
}

#[derive(Component)]
struct SpadePart;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Part {
    Host,
    Toad,
    Mario,
    Box,
    Icons,
    /// Row, piece (0: from its x, 1: wrapped round).
    Row(usize, usize),
    Prize,
    Frame,
    Shade,
}

/// Camera-local transform of something `w` x `h` whose top-left is at GBA screen (x, y).
fn at(x: f32, y: f32, w: f32, h: f32, z: f32) -> Transform {
    Transform::from_xyz(x + w / 2.0 - SCREEN_W as f32 / 2.0, SCREEN_H as f32 / 2.0 - y - h / 2.0, z)
}

fn load(dir: &std::path::Path, name: &str, images: &mut Assets<Image>) -> Option<Handle<Image>> {
    match Rgba::read_png(&spade_path(dir, name)) {
        Ok(img) => Some(images.add(sprite_image(img))),
        Err(e) => {
            warn!("{e}; re-run sma4-extract");
            None
        }
    }
}

fn load_art(
    mut commands: Commands,
    dir: Res<crate::AssetDir>,
    mut game: ResMut<GameRes>,
    mut images: ResMut<Assets<Image>>,
    camera: Single<Entity, With<Camera2d>>,
) {
    let asset = match SpadeAsset::read(&dir.0) {
        Ok(a) => a,
        Err(e) => {
            warn!("{e}; re-run sma4-extract for the spade game");
            return;
        }
    };
    let dir = &dir.0;
    let mut get = |name: &str| load(dir, name, &mut images);
    let (Some(host), Some(toad), Some(message_box), Some(frame)) =
        (get(SPADE_HOST), get(SPADE_HOST_TOAD), get(SPADE_HOST_BOX), get(SPADE_FRAME))
    else {
        return;
    };
    let texts: Option<Vec<_>> = (0..SPADE_MESSAGES).map(|k| get(&spade_host_text(k))).collect();
    let reels: Option<Vec<_>> = (0..SPADE_COUNTS).map(|c| get(&spade_reels(c))).collect();
    let prizes: Option<Vec<_>> =
        asset.prizes.iter().map(|&(p, x, y)| get(&spade_prize(p)).map(|h| (p, h, (x, y)))).collect();
    let (Some(texts), Some(reels), Some(prizes)) = (texts, reels, prizes) else { return };
    let tiles = match Rgba::read_png(&spade_path(dir, SPADE_TILES)) {
        Ok(t) => t,
        Err(e) => {
            warn!("{e}; re-run sma4-extract");
            return;
        }
    };
    let tile_index = asset.tile_keys.iter().enumerate().map(|(i, &e)| (e, i)).collect();
    let icons = images.add(Image::new(
        Extent3d { width: SCREEN_W, height: SCREEN_H, depth_or_array_layers: 1 },
        TextureDimension::D2,
        vec![0; (SCREEN_W * SCREEN_H * 4) as usize],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    let stand = sma4_assets::PLAYER_WALK + sma4_core::player::STAND_STEP as usize;
    let mario = (0..4u8)
        .filter_map(|form| load_cells(&sma4_assets::player_path(dir, form), PLAYER_W, PLAYER_H, &mut images).get(stand).cloned())
        .collect();

    let full = |z| at(0.0, 0.0, SCREEN_W as f32, SCREEN_H as f32, z);
    commands.entity(*camera).with_children(|cam| {
        let mut part = |p: Part, sprite: Sprite, t: Transform| {
            cam.spawn((SpadePart, p, sprite, t, Visibility::Hidden));
        };
        part(Part::Host, Sprite::from_image(host), full(Z_BACK));
        part(Part::Toad, Sprite::from_image(toad), full(Z_SPRITES));
        part(Part::Mario, Sprite::default(), Transform::default());
        part(Part::Box, Sprite::default(), Transform::default());
        part(Part::Icons, Sprite::from_image(icons.clone()), full(Z_BACK));
        for row in 0..3 {
            for piece in 0..2 {
                part(Part::Row(row, piece), Sprite::default(), Transform::default());
            }
        }
        part(Part::Prize, Sprite::default(), Transform::default());
        part(Part::Frame, Sprite::from_image(frame), full(Z_BOX));
        part(Part::Shade, Sprite { color: Color::NONE, custom_size: Some(Vec2::splat(4096.0)), ..default() }, full(Z_SHADE));
    });
    game.0.spade_data = Some(asset.clone());
    commands.insert_resource(Art {
        asset,
        message_box,
        texts,
        reels,
        tiles,
        tile_index,
        icons,
        drawn: None,
        prizes,
        mario,
    });
}

/// `--spade`: Mario on the map's spade spot, its game started.
fn debug_start(mut debug: ResMut<DebugSpade>, mut game: ResMut<GameRes>) {
    let Some(count) = debug.0.take() else { return };
    let g = &mut game.0;
    g.spade_streak.count = count;
    let Some(map) = &mut g.world_map else { return };
    let Some(node) = map.nodes.iter().copied().find(|n| n.tileset == sma4_core::spade::TILESET) else { return };
    (map.y, map.x) = (node.y, node.x);
    if g.enter_spade(&node) {
        info!("debug: spade game, streak count {count}");
    }
}

/// Where a BG3 map cell (32 wide) lands on screen with the room's scroll.
fn bg3_cell(index: u32, (sx, sy): (u16, u16)) -> (i32, i32) {
    let (c, r) = ((index % 32) as i32, (index / 32) as i32);
    ((c * 8 - sx as i32).rem_euclid(256), (r * 8 - sy as i32).rem_euclid(512))
}

/// BG2 from its map entries, over the backdrop.
fn draw_icons(art: &Art, map: &[u16], image: &mut Image) {
    let Some(data) = image.data.as_mut() else { return };
    let n = art.tiles.width as usize / 8;
    for (i, px) in data.chunks_mut(4).enumerate() {
        let (x, y) = (i % SCREEN_W as usize, i / SCREEN_W as usize);
        let entry = map.get((y / 8) * 32 + x / 8).copied().unwrap_or(0);
        let rgba = art.tile_index.get(&entry).map(|&t| {
            let o = ((y % 8) * n * 8 + t * 8 + x % 8) * 4;
            &art.tiles.pixels[o..o + 4]
        });
        match rgba {
            Some(c) if c[3] != 0 => px.copy_from_slice(c),
            _ => px.copy_from_slice(&art.asset.backdrop),
        }
    }
}

fn sync_spade(
    game: Res<GameRes>,
    art: Option<ResMut<Art>>,
    mut images: ResMut<Assets<Image>>,
    mut parts: Query<(&Part, &mut Sprite, &mut Transform, &mut Visibility), With<SpadePart>>,
    mut texts: Local<Vec<Entity>>,
    mut commands: Commands,
    camera: Single<Entity, With<Camera2d>>,
) {
    for e in texts.drain(..) {
        commands.entity(e).despawn();
    }
    let Some(mut art) = art else { return };
    let spade = match (&game.0.screen, &game.0.spade) {
        (Screen::Spade, Some(s)) => Some(s),
        _ => None,
    };
    let slots = spade.and_then(|s| s.slots.as_ref().filter(|_| matches!(s.phase, Phase::FadeIn(_) | Phase::Slots)));
    let in_room = spade.is_some() && slots.is_none();
    if let Some(s) = slots
        && art.drawn != Some((s.count, s.bg2_generation))
    {
        art.drawn = Some((s.count, s.bg2_generation));
        let handle = art.icons.clone();
        if let Some(mut image) = images.get_mut(&handle) {
            draw_icons(&art, &s.bg2, &mut image);
        }
    }
    let form = game.0.player.form.id() as usize;
    for (part, mut sprite, mut t, mut vis) in &mut parts {
        let shown = match *part {
            Part::Host | Part::Toad => in_room,
            Part::Mario => {
                if let Some(image) = art.mario.get(form) {
                    sprite.image = image.clone();
                }
                // Facing right: the art (facing left) is h-flipped.
                sprite.flip_x = true;
                let (x, y) = HOST_MARIO_OAM;
                *t = at(x as f32, y as f32, PLAYER_W as f32, PLAYER_H as f32, Z_SPRITES);
                in_room
            }
            Part::Box => {
                let rows = spade.map_or(0, |s| s.box_rows()).min(SPADE_BOX_ROWS);
                let (x, y) = bg3_cell(SPADE_BOX_AT.1 * 32 + SPADE_BOX_AT.0, art.asset.host_scroll);
                let (w, h) = (SPADE_BOX_W * 8, rows * 8);
                sprite.image = art.message_box.clone();
                sprite.rect = Some(Rect::new(x as f32, y as f32, (x + w as i32) as f32, (y + h as i32) as f32));
                *t = at(x as f32, y as f32, w as f32, h as f32, Z_BOX);
                in_room && rows > 0
            }
            Part::Icons => slots.is_some(),
            Part::Row(row, piece) => match slots {
                Some(s) => {
                    let (y0, y1) = ROW_LINES[row];
                    let sx = (s.reels[row].scroll() & 0x1FF) as u32;
                    // The layer is 512 px wide: the part past its right edge wraps round.
                    let (src_x, w, dst_x) = if piece == 0 {
                        (sx, (512 - sx).min(SCREEN_W), 0)
                    } else {
                        (0, SCREEN_W.saturating_sub(512 - sx), 512 - sx)
                    };
                    let src_y = (y0 as i32 + REELS_VOFS).max(0) as u32;
                    let h = y1 - y0;
                    sprite.image = art.reels[(s.count as usize).min(art.reels.len() - 1)].clone();
                    sprite.rect = Some(Rect::new(src_x as f32, src_y as f32, (src_x + w) as f32, (src_y + h) as f32));
                    *t = at(dst_x as f32, y0 as f32, w as f32, h as f32, Z_PICTURES);
                    w > 0
                }
                None => false,
            },
            Part::Prize => match slots.filter(|s| s.prize_shown) {
                Some(s) => match art.prizes.iter().find(|p| p.0 == s.prize) {
                    Some((_, image, (dx, dy))) => {
                        // OAM y wraps at 256: 160 and up are above the screen.
                        let y = (s.prize_y as i32 + dy) & 0xFF;
                        let y = if y >= SCREEN_H as i32 { y - 256 } else { y };
                        let size = images.get(image).map_or(Vec2::ZERO, |i| i.size_f32());
                        sprite.image = image.clone();
                        *t = at((PRIZE_X + dx) as f32, y as f32, size.x, size.y, Z_SPRITES);
                        true
                    }
                    None => false,
                },
                None => false,
            },
            Part::Frame => slots.is_some(),
            Part::Shade => match spade.and_then(Spade::shade) {
                Some(shade) => {
                    // Brightness down (BLDY) scales every colour by (16 - y) / 16: black at y / 16
                    // over it. The palette fade takes the same off every channel; drawn alike.
                    let a = match shade {
                        Shade::Darken(y) => y as f32 / 16.0,
                        Shade::Subtract(n) => n as f32 / 31.0,
                    };
                    sprite.color = Color::srgba(0.0, 0.0, 0.0, a.min(1.0));
                    true
                }
                None => false,
            },
        };
        *vis = if shown { Visibility::Inherited } else { Visibility::Hidden };
    }
    // The message, one cell per printed letter.
    if let Some(s) = spade.filter(|_| in_room) {
        let k = sma4_core::spade::Spade::message_index(game.0.spade_streak);
        let Some(image) = art.texts.get(k) else { return };
        commands.entity(*camera).with_children(|cam| {
            for (col, row) in s.text_cells() {
                let (x, y) = bg3_cell(SPADE_TEXT_AT + row * 32 + col, art.asset.host_scroll);
                let mut sprite = Sprite::from_image(image.clone());
                sprite.rect = Some(Rect::new(x as f32, y as f32, (x + 8) as f32, (y + 8) as f32));
                texts.push(cam.spawn((sprite, at(x as f32, y as f32, 8.0, 8.0, Z_TEXT))).id());
            }
        });
    }
}

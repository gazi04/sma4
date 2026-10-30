//! The title screen QUIT goes back to. `sma4_core::title` keeps the GBA's video state as
//! the ROM drives it (VRAM, palette, OAM, scroll, brightness); this draws that state the
//! way the GBA's display would, into one 240x160 picture a frame: the text BGs and
//! sprites by priority, then BLDY's brightness change.

use std::sync::Arc;

use bevy::prelude::*;
use sma4_assets::{Rgba, TitleAsset};
use sma4_core::title::Video;
use sma4_core::{SCREEN_H, SCREEN_W, Screen};

use crate::GameRes;
use crate::player::sprite_image;

pub struct TitlePlugin;

impl Plugin for TitlePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, load_title).add_systems(Update, (debug_start, sync_title).chain());
    }
}

/// Debug: start on the title screen (`--title`).
#[derive(Resource, Default)]
pub struct DebugTitle(pub bool);

fn debug_start(mut debug: ResMut<DebugTitle>, mut game: ResMut<GameRes>) {
    if std::mem::take(&mut debug.0) {
        game.0.show_title();
        info!("debug: title screen");
    }
}

/// Over the off-level cover (z 40) like the map.
const Z_TITLE: f32 = 41.0;

#[derive(Component)]
struct TitleView(Handle<Image>);

fn load_title(
    mut commands: Commands,
    dir: Res<crate::AssetDir>,
    mut game: ResMut<GameRes>,
    mut images: ResMut<Assets<Image>>,
    camera: Single<Entity, With<Camera2d>>,
) {
    let asset = match TitleAsset::read(&dir.0) {
        Ok(a) => a,
        Err(e) => {
            warn!("{e}; re-run sma4-extract for the title screen");
            return;
        }
    };
    game.0.title_data = Some(Arc::new(asset));
    let image = images.add(sprite_image(Rgba { width: SCREEN_W, height: SCREEN_H, pixels: vec![0; (SCREEN_W * SCREEN_H * 4) as usize] }));
    commands.entity(*camera).with_children(|cam| {
        cam.spawn((TitleView(image.clone()), Sprite::from_image(image), Transform::from_xyz(0.0, 0.0, Z_TITLE), Visibility::Hidden));
    });
}

fn sync_title(game: Res<GameRes>, mut images: ResMut<Assets<Image>>, mut view: Query<(&TitleView, &mut Visibility)>) {
    let title = match (&game.0.screen, &game.0.title) {
        (Screen::Title, Some(t)) => Some(t),
        _ => None,
    };
    for (v, mut vis) in &mut view {
        let Some(t) = title else {
            *vis = Visibility::Hidden;
            continue;
        };
        if let Some(mut image) = images.get_mut(&v.0)
            && let Some(data) = image.data.as_mut()
        {
            render(&t.video, data);
        }
        *vis = Visibility::Inherited;
    }
}

/// OBJ sizes by shape and size.
const OBJ_SIZES: [[(i32, i32); 4]; 3] = [
    [(8, 8), (16, 16), (32, 32), (64, 64)],
    [(16, 8), (32, 8), (32, 16), (64, 32)],
    [(8, 16), (8, 32), (16, 32), (32, 64)],
];

/// The top two opaque pixels' (colour, layer bit, priority) on one line, BGs and OBJs.
#[derive(Clone, Copy)]
struct Px {
    colour: u16,
    /// BLDCNT target bit: BG0-3 = 0-3, OBJ 4, backdrop 5.
    layer: u8,
    prio: u8,
}

fn u16_at(d: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([d[at], d[at + 1]])
}

/// A 4bpp colour index from VRAM.
fn nibble(vram: &[u8], tile_at: usize, x: usize, y: usize) -> u8 {
    let b = vram.get(tile_at + y * 4 + x / 2).copied().unwrap_or(0);
    if x & 1 == 1 { b >> 4 } else { b & 0xF }
}

/// The opaque BG pixel of BG `bg` at screen (x, y), if any.
fn bg_pixel(v: &Video, bg: usize, x: i32, y: i32) -> Option<u16> {
    let cnt = v.bgcnt[bg] as usize;
    let (w, h) = [(256, 256), (512, 256), (256, 512), (512, 512)][cnt >> 14];
    let (sx, sy) = v.scroll[bg];
    let lx = (x + sx as i32).rem_euclid(w);
    let ly = (y + sy as i32).rem_euclid(h);
    let (tx, ty) = ((lx / 8) as usize, (ly / 8) as usize);
    let block = tx / 32 + (ty / 32) * (w as usize / 256);
    let at = (cnt >> 8 & 0x1F) * 0x800 + block * 0x800 + ((ty % 32) * 32 + tx % 32) * 2;
    let e = u16_at(&v.vram, at) as usize;
    let (mut px, mut py) = ((lx % 8) as usize, (ly % 8) as usize);
    if e & 0x400 != 0 {
        px = 7 - px;
    }
    if e & 0x800 != 0 {
        py = 7 - py;
    }
    let tile_at = (cnt >> 2 & 3) * 0x4000 + (e & 0x3FF) * 32;
    let c = nibble(&v.vram, tile_at, px, py);
    (c != 0).then(|| v.palette[(e >> 12) * 16 + c as usize])
}

/// One line of sprites: for each x the frontmost opaque OBJ pixel (colour, priority).
fn obj_line(v: &Video, y: i32, line: &mut [Option<(u16, u8)>]) {
    line.fill(None);
    // Lower OAM entries are in front: draw from the back.
    for s in (0..128).rev() {
        let e = &v.oam[s * 8..s * 8 + 8];
        let (a0, a1, a2) = (u16_at(e, 0), u16_at(e, 2), u16_at(e, 4));
        let affine = a0 & 0x100 != 0;
        if (!affine && a0 & 0x200 != 0) || a0 >> 14 == 3 || (a0 >> 10 & 3) == 2 {
            continue;
        }
        let (w, h) = OBJ_SIZES[(a0 >> 14) as usize][(a1 >> 14) as usize];
        let double = affine && a0 & 0x200 != 0;
        let (bw, bh) = if double { (w * 2, h * 2) } else { (w, h) };
        let row = (y - (a0 & 0xFF) as i32).rem_euclid(256);
        if row >= bh {
            continue;
        }
        let x0 = if a1 & 0x100 != 0 { (a1 & 0x1FF) as i32 - 512 } else { (a1 & 0x1FF) as i32 };
        let prio = (a2 >> 10 & 3) as u8;
        let pal = (a2 >> 12) as usize;
        let tile = (a2 & 0x3FF) as usize;
        let params = if affine {
            let g = (a1 >> 9 & 0x1F) as usize * 4;
            let p = |k: usize| u16_at(&v.oam, (g + k) * 8 + 6) as i16 as i32;
            Some((p(0), p(1), p(2), p(3)))
        } else {
            None
        };
        for col in 0..bw {
            let sx = x0 + col;
            if !(0..SCREEN_W as i32).contains(&sx) {
                continue;
            }
            let (tx, ty) = match params {
                Some((pa, pb, pc, pd)) => {
                    let (dx, dy) = (col - bw / 2, row - bh / 2);
                    (((pa * dx + pb * dy) >> 8) + w / 2, ((pc * dx + pd * dy) >> 8) + h / 2)
                }
                None => (
                    if a1 & 0x1000 != 0 { w - 1 - col } else { col },
                    if a1 & 0x2000 != 0 { h - 1 - row } else { row },
                ),
            };
            if !(0..w).contains(&tx) || !(0..h).contains(&ty) {
                continue;
            }
            // 2D mapping: 32 tiles to a row.
            let t = tile + (ty as usize / 8) * 32 + tx as usize / 8;
            let c = nibble(&v.vram, 0x1_0000 + (t & 0x3FF) * 32, tx as usize % 8, ty as usize % 8);
            if c != 0 {
                line[sx as usize] = Some((v.palette[0x100 + pal * 16 + c as usize], prio));
            }
        }
    }
}

/// BLDY's brightness change on one 5-bit channel.
fn bright(c: u16, mode: u16, y: u16) -> u16 {
    let y = y.min(16);
    match mode {
        2 => c + (((31 - c) * y) >> 4),
        3 => c - ((c * y) >> 4),
        _ => c,
    }
}

fn render(v: &Video, out: &mut [u8]) {
    let mut objs = vec![None; SCREEN_W as usize];
    let mode = v.bldcnt >> 6 & 3;
    for y in 0..SCREEN_H as i32 {
        if v.dispcnt & 0x1000 != 0 {
            obj_line(v, y, &mut objs);
        } else {
            objs.fill(None);
        }
        for x in 0..SCREEN_W as i32 {
            let mut top = Px { colour: v.palette[0], layer: 5, prio: 4 };
            for bg in (0..4).rev() {
                if v.dispcnt & (0x100 << bg) == 0 {
                    continue;
                }
                let prio = (v.bgcnt[bg] & 3) as u8;
                if prio <= top.prio
                    && let Some(c) = bg_pixel(v, bg, x, y)
                {
                    top = Px { colour: c, layer: bg as u8, prio };
                }
            }
            if let Some((c, prio)) = objs[x as usize]
                && prio <= top.prio
            {
                top = Px { colour: c, layer: 4, prio };
            }
            let mut c = top.colour;
            if mode >= 2 && v.bldcnt >> top.layer & 1 != 0 {
                let ch = |s: u16| bright(c >> s & 0x1F, mode, v.bldy);
                c = ch(0) | ch(5) << 5 | ch(10) << 10;
            }
            let e = |s: u16| {
                let k = (c >> s & 0x1F) as u8;
                k << 3 | k >> 2
            };
            let o = ((y as u32 * SCREEN_W + x as u32) * 4) as usize;
            out[o..o + 4].copy_from_slice(&[e(0), e(5), e(10), 255]);
        }
    }
}

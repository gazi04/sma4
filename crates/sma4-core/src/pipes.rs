//! Pipes into another area of the level (World 1-1's coin room): the entry checks, the
//! slow slide in and out, and where the player comes out.
//!
//! * Entry 0x0809B598: down held while standing on, or up held while bumping into, the
//!   end of a pipe (metatile 0xAD / 0xAE; 0xAF / 0xB0 lead elsewhere) close enough to its
//!   middle. 0x0809BCB8 starts the move: +0x4A = direction | kind << 2, timer +0x64 = 0x3C.
//! * Move 0x080A8000 -> 0x080A9064: y speed 0x0817388A[dir & 1] = +8 / -8 (half a pixel a
//!   frame), drawn behind the pipe (0x0809BA58). When the timer runs out, kind 1 switches
//!   the area (0x03003AE7 + 1 -> 0x08005690).
//! * Area switch 0x0800515C / 0x08005290: the header's alternate level; the player's
//!   position from the old area's pointer for the page the pipe was entered on.
//! * End pipes (kind 4): where the tileset's class has pipe flag bit 7 (0x08172E44), the
//!   ends 0xBD / 0xBE lead to the world's end room (transition 4, 0x080056A0: level
//!   0x08131EDA[world], list 0x08131EEA, tileset 0x08131EFA; frame (0x28, 0x180), up out
//!   of a pipe, camera y 0xF0). Seen in mGBA on World 1-5's last pipe: +0x4A = 0x13.
//! * Exit 0x0809F2B8 -> 0x080A866C: up (0x080A86F4: 0x3F frames at -8) or down (0x080A8764:
//!   7 frames at +8, then falling at 0x38).

use sma4_assets::{EndRoom, PipeExit};

use crate::Buttons;
use crate::level::{Level, TILE_PX};
use crate::player::{Form, Player};

/// Pipe-end metatiles that lead to the other area (0x0809B598: 0xB0 - id >> 1 == 1).
const PIPE_LEFT: u8 = 0xAD;
const PIPE_RIGHT: u8 = 0xAE;
/// End-of-level pipe ends, and the class flag that enables them.
const END_LEFT: u8 = 0xBD;
const END_RIGHT: u8 = 0xBE;
pub const END_PIPES: u8 = 0x80;
/// Entry timer (0x0809BCB8: +0x64 = 0x3C).
pub const ENTER_FRAMES: u8 = 0x3C;
/// Exit timers 0x08173018[dir]: up 0x3F, down 7.
const EXIT_UP_FRAMES: u8 = 0x3F;
const EXIT_DOWN_FRAMES: u8 = 7;
/// Half a pixel a frame (speed byte 8, in 1/16 px), in 1/256 px.
const SLIDE: i32 = 8 * 16;
/// Fall speed after coming down out of a pipe (0x080A8764: +0x1D = 0x38).
const DROP_SPEED: i32 = 0x38 * 16;
/// How far into the pipe the frame's left edge may be (0x0809B598's alignment test with
/// 0x08172E52[i] = 4 on the ground, 8 in the air): px from the pipe's left edge.
const DOWN_WINDOW: std::ops::RangeInclusive<i32> = 3..=11;
const UP_WINDOW: std::ops::RangeInclusive<i32> = 3..=12;

/// Direction byte +0x4A / 0x03003BF1: 1 up, 2 down.
pub const UP: u8 = 1;
pub const DOWN: u8 = 2;
/// Arriving from below by the sky flight (World 1-5's pointer to its coin heaven): no exit
/// animation (0x08173018[7] = 0).
pub const SKY_EXIT: u8 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pipe {
    /// Sliding into the pipe.
    Enter { dir: u8, timer: u8 },
    /// In the pipe while the host loads the other area.
    Switching,
    /// Coming out in the new area.
    Exit { dir: u8, timer: u8 },
}

/// Where the area switch goes: the other area, and the player's frame and camera there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AreaRequest {
    pub level: u16,
    pub enemies: u16,
    pub tileset: u8,
    pub exit: PipeExit,
}

/// The pipe end at `(tx, ty)` (its left column): Some(true) for an end-of-level pipe.
fn pipe_end(level: &Level, tx: i32, ty: i32) -> Option<bool> {
    match (level.collision_id(tx, ty)?, level.collision_id(tx + 1, ty)?) {
        (PIPE_LEFT, PIPE_RIGHT) if level.junction.is_some() => Some(false),
        (END_LEFT, END_RIGHT) if level.pipe_flags & END_PIPES != 0 => Some(true),
        _ => None,
    }
}

/// Left edge of the player's 16 px frame (`Player::frame`, the ROM's x).
fn frame_x(p: &Player) -> i32 {
    p.frame().0
}

/// The pipe the player is lined up with: its left column, if the frame's left edge is
/// within `window` px of the pipe's left edge.
fn lined_up(level: &Level, p: &Player, ty: i32, window: std::ops::RangeInclusive<i32>) -> Option<bool> {
    let x = frame_x(p);
    let tx = (x - window.start()).div_euclid(TILE_PX);
    if window.contains(&(x - tx * TILE_PX)) { pipe_end(level, tx, ty) } else { None }
}

/// 0x0809B598: down held standing on a pipe end, or up held with the head against one:
/// the direction, and whether it is an end-of-level pipe. Only areas with somewhere to go
/// have working junction pipes.
pub fn entry(level: &Level, p: &Player, held: Buttons) -> Option<(u8, bool)> {
    let (_, py) = p.px();
    if p.on_ground && held.contains(Buttons::DOWN) {
        let feet = (py + p.height()).div_euclid(TILE_PX);
        return lined_up(level, p, feet, DOWN_WINDOW).map(|end| (DOWN, end));
    }
    // The head against the tile above (+0x72): `Player::move_y` stops it on the tile
    // boundary, and it stays in that pixel for the first frames of the fall, so up may
    // come a little after the jump.
    if !p.on_ground && held.contains(Buttons::UP) && py.rem_euclid(TILE_PX) == 0 {
        let head = py.div_euclid(TILE_PX) - 1;
        return lined_up(level, p, head, UP_WINDOW).map(|end| (UP, end));
    }
    None
}

/// Where an end-of-level pipe leads: the world's end room (0x080056A0).
pub fn end_room(room: &EndRoom) -> AreaRequest {
    let (x, y) = sma4_assets::END_ROOM_ARRIVAL;
    let exit = PipeExit { page: 0, x, y, dir: UP, camera_y: sma4_assets::END_ROOM_CAMERA_Y };
    AreaRequest { level: room.level, enemies: room.enemies, tileset: room.tileset, exit }
}

/// Where the pipe the player is in leads: the pointer of the page it was entered on.
pub fn destination(level: &Level, p: &Player) -> Option<AreaRequest> {
    let j = level.junction.as_ref()?;
    let page = frame_x(p).div_euclid(256);
    let exit = *j.exits.iter().find(|e| e.page as i32 == page)?;
    Some(AreaRequest { level: j.level, enemies: j.enemies, tileset: j.tileset, exit })
}

/// The player placed in the pipe it comes out of: the pointer gives the 16x32 frame's
/// top-left.
pub fn place(p: &mut Player, exit: &PipeExit) {
    let form = p.form;
    let facing_left = p.facing_left;
    *p = Player::spawn(exit.x as i32 + 2, exit.y as i32 + 32 - Form::Small.height());
    p.set_form(form);
    p.facing_left = facing_left;
}

impl Pipe {
    pub fn enter(dir: u8) -> Self {
        Pipe::Enter { dir, timer: ENTER_FRAMES }
    }

    pub fn exit(dir: u8) -> Self {
        let timer = if dir == DOWN { EXIT_DOWN_FRAMES } else { EXIT_UP_FRAMES };
        Pipe::Exit { dir, timer }
    }

    /// One frame. Returns the pipe's next state (None once the player is out).
    pub fn step(self, p: &mut Player) -> Option<Self> {
        match self {
            Pipe::Enter { dir, timer } | Pipe::Exit { dir, timer } => {
                p.vx = 0;
                p.vy = 0;
                p.on_ground = false;
                p.y += if dir == UP { -SLIDE } else { SLIDE };
                let timer = timer - 1;
                if timer != 0 {
                    return Some(match self {
                        Pipe::Enter { .. } => Pipe::Enter { dir, timer },
                        _ => Pipe::Exit { dir, timer },
                    });
                }
                match self {
                    Pipe::Enter { .. } => Some(Pipe::Switching),
                    _ => {
                        if dir == DOWN {
                            p.vy = DROP_SPEED;
                        }
                        None
                    }
                }
            }
            Pipe::Switching => Some(Pipe::Switching),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Game;
    use crate::level::{SKY, TEST_SOLIDITY};
    use crate::player::SUB;
    use sma4_assets::{BlockRules, Junction};

    const W: usize = 20;
    const H: usize = 14;

    /// Floor on the last row; a pipe end (0xAD, 0xAE) at columns 4-5 of `row`.
    fn area(id: u16, row: usize, to: u16, exit: PipeExit) -> Level {
        let mut m = vec![SKY; W * H];
        for x in 0..W {
            m[(H - 1) * W + x] = 0x53;
        }
        m[row * W + 4] = PIPE_LEFT as u16;
        m[row * W + 5] = PIPE_RIGHT as u16;
        let junction = Junction { level: to, enemies: 0, tileset: 1, exits: vec![exit] };
        Level::new(W, H, m, TEST_SOLIDITY, BlockRules::default()).with_area(id, Some(junction), None)
    }

    fn run(g: &mut Game, held: Buttons, frames: u32) {
        for _ in 0..frames {
            g.step(held);
        }
    }

    /// 1-1 -> coin room -> 1-1: down into a pipe, out of the room's ceiling pipe, up into
    /// the room's other pipe from below, out on top of a pipe back in the level.
    #[test]
    fn a_pipe_leads_to_the_other_area_and_back() {
        // The level: a pipe standing on the floor, top at row 11 (y 176).
        let into_room = PipeExit { page: 0, x: 0xC8, y: 0x10, dir: DOWN, camera_y: 0 };
        let level = area(0x22, 11, 0x23, into_room);
        let mut g = Game::new();
        // Frame left edge 7 px into the pipe (x 64 + 7): hitbox at +2.
        g.enter_level(level, (4 * 16 + 7 + 2, 176 - 16), &[]);
        run(&mut g, Buttons::empty(), 2);
        assert!(g.player.on_ground);
        let y0 = g.player.y;

        g.step(Buttons::DOWN);
        assert_eq!(g.pipe, Some(Pipe::enter(DOWN)));
        run(&mut g, Buttons::DOWN, ENTER_FRAMES as u32);
        assert_eq!(g.pipe, Some(Pipe::Switching));
        assert_eq!(g.player.y - y0, ENTER_FRAMES as i32 * SLIDE, "half a pixel a frame");
        let to = g.pending_area.expect("the pipe leads somewhere");
        assert_eq!((to.level, to.exit), (0x23, into_room));
        // Nothing moves while the host loads the room.
        run(&mut g, Buttons::empty(), 3);
        assert_eq!(g.pipe, Some(Pipe::Switching));

        // The room: its exit pipe hangs from the ceiling, end at row 8, leading back on
        // page 0.
        let back = PipeExit { page: 0, x: 0x108, y: 0xA0, dir: UP, camera_y: 0 };
        let room = area(0x23, 8, 0x22, back);
        g.pending_area = None;
        g.enter_area(room, &[], &to);
        assert_eq!(g.level.id, 0x23);
        assert_eq!(g.player.frame(), (0xC8, 0x10));
        // Down out of the ceiling pipe: 7 frames sliding, then falling.
        run(&mut g, Buttons::empty(), EXIT_DOWN_FRAMES as u32);
        assert_eq!(g.pipe, None);
        assert_eq!(g.player.vy, DROP_SPEED);
        run(&mut g, Buttons::empty(), 60);
        assert!(g.player.on_ground);

        // Walk under the exit pipe (frame left edge 5 px into it) and jump holding up.
        g.player.x = (4 * 16 + 5 + 2) * SUB;
        let mut entered = false;
        for _ in 0..40 {
            g.step(Buttons::A | Buttons::UP);
            if g.pipe.is_some() {
                entered = true;
                break;
            }
        }
        assert!(entered, "jumping into the pipe from below with up held");
        assert_eq!(g.pipe, Some(Pipe::enter(UP)));
        run(&mut g, Buttons::empty(), ENTER_FRAMES as u32);
        let to = g.pending_area.take().expect("back to the level");
        assert_eq!((to.level, to.exit), (0x22, back));

        g.enter_area(area(0x22, 11, 0x23, into_room), &[], &to);
        let y = g.player.y;
        run(&mut g, Buttons::empty(), EXIT_UP_FRAMES as u32);
        assert_eq!(g.pipe, None);
        assert_eq!(g.player.y - y, -(EXIT_UP_FRAMES as i32) * SLIDE);
    }

    /// World 1-5's orange note block: the sky flight leaves the top of the area and the
    /// page's pointer (exit direction 7) puts the player at the bottom of the other area,
    /// still rising.
    #[test]
    fn the_sky_flight_leaves_through_the_top_into_the_other_area() {
        let sky = PipeExit { page: 0, x: 0x78, y: 0x180, dir: SKY_EXIT, camera_y: 0xF0 };
        let level = area(0x82, 11, 0x154, sky);
        let mut g = Game::new();
        g.enter_level(level, (2 * 16, 176 - 16), &[]);
        run(&mut g, Buttons::empty(), 2);
        g.player.sky = crate::player::SKY_LAUNCH;
        let mut frames = 0;
        while g.pending_area.is_none() {
            g.step(Buttons::empty());
            frames += 1;
            assert!(frames < 100, "flies up out of the area");
            assert_eq!(g.player.vx, 0);
        }
        let to = g.pending_area.take().unwrap();
        assert_eq!((to.level, to.exit), (0x154, sky));
        g.enter_area(area(0x154, 11, 0x82, sky), &[], &to);
        assert_eq!(g.player.frame(), (0x78, 0x180));
        assert_eq!(g.pipe, None);
        let y = g.player.y;
        run(&mut g, Buttons::empty(), 10);
        assert!(g.player.y < y, "still rising");
    }

    /// World 1-5's last pipe: an end-of-level pipe end (0xBD / 0xBE) hanging from the
    /// ceiling, in a tileset whose class allows them, leads to the world's end room.
    #[test]
    fn end_of_level_pipes_lead_to_the_worlds_end_room() {
        let mut m = vec![SKY; W * H];
        m[(H - 1) * W..].fill(0x53);
        m[8 * W + 4] = END_LEFT as u16;
        m[8 * W + 5] = END_RIGHT as u16;
        let level = Level::new(W, H, m, TEST_SOLIDITY, BlockRules::default());
        let mut g = Game::new();
        let room = EndRoom { level: 0x27, enemies: 0x52, tileset: 1 };
        g.end_rooms = Some([room; sma4_assets::WORLDS]);
        g.enter_level(level.clone(), (4 * 16 + 5 + 2, (H as i32 - 1) * 16 - 16), &[]);
        run(&mut g, Buttons::empty(), 2);
        for _ in 0..40 {
            g.step(Buttons::A | Buttons::UP);
        }
        assert!(g.pipe.is_none(), "not without the class flag");
        g.enter_level(level.with_pipe_flags(END_PIPES), (4 * 16 + 5 + 2, (H as i32 - 1) * 16 - 16), &[]);
        run(&mut g, Buttons::empty(), 2);
        for _ in 0..40 {
            g.step(Buttons::A | Buttons::UP);
            if g.pipe.is_some() {
                break;
            }
        }
        assert_eq!(g.pipe, Some(Pipe::enter(UP)));
        run(&mut g, Buttons::empty(), ENTER_FRAMES as u32);
        let to = g.pending_area.take().expect("the end room");
        assert_eq!((to.level, to.enemies, to.tileset), (0x27, 0x52, 1));
        assert_eq!((to.exit.x, to.exit.y, to.exit.dir), (0x28, 0x180, UP));
    }

    /// The room's exit pipe hangs 6 px above the raccoon's head: the head hits it on the
    /// second frame of the jump. Up pressed a couple of frames after the jump still works.
    #[test]
    fn up_may_come_just_after_the_jump() {
        let exit = PipeExit { page: 0, x: 0, y: 0, dir: UP, camera_y: 0 };
        // Pipe end at row 8 (bottom y 144); a ledge at row 11 (top y 176), 32 px lower.
        let mut level = area(0x23, 8, 0x22, exit);
        for x in 0..W as i32 {
            level.set(x, 11, 0x53);
        }
        for late in 0..4 {
            let mut g = Game::new();
            g.enter_level(level.clone(), (4 * 16 + 7 + 2, 176 - 16), &[]);
            g.player.set_form(Form::Raccoon);
            run(&mut g, Buttons::empty(), 2);
            let mut entered = false;
            for f in 0..30 {
                g.step(if f < late { Buttons::A } else { Buttons::A | Buttons::UP });
                entered |= g.pipe.is_some();
            }
            assert!(entered, "up {late} frames after the jump");
        }
    }

    #[test]
    fn pipes_need_lining_up_and_somewhere_to_go() {
        let exit = PipeExit { page: 0, x: 0, y: 0, dir: DOWN, camera_y: 0 };
        let mut p = Player::spawn(0, 176 - 16);
        p.on_ground = true;
        let level = area(0x22, 11, 0x23, exit);
        for (left, ok) in [(2, false), (3, true), (11, true), (12, false), (20, false)] {
            p.x = (4 * 16 + left + 2) * SUB;
            assert_eq!(entry(&level, &p, Buttons::DOWN).is_some(), ok, "frame {left} px into the pipe");
        }
        p.x = (4 * 16 + 7 + 2) * SUB;
        assert_eq!(entry(&level, &p, Buttons::RIGHT), None, "down must be held");
        let mut shut = level;
        shut.junction = None;
        assert_eq!(entry(&shut, &p, Buttons::DOWN), None, "no other area");
    }
}

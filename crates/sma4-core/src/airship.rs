//! The airship: its intro (level 0x202, header start kind 6) and its landing (level 0x205,
//! kind 7). See docs/re-notes.md "King's castle".
//!
//! The intro is the player's scripted mode +0x45 (0x0809F1FC -> 0x0809F400 sets it to 1,
//! the BG camera y 0x03003754 to -5); every frame 0x080A8000 moves the camera x on by 1,
//! and once it is past 0x5F counts +0x3A up and steps the BG camera y down whenever +0x3A +
//! 0x03003CED carries (0x03003CED = (it & 0xF0) + 0x70 each frame, 0x08094780): the
//! airship's hull (the level's top rows, BG1) rises, faster and faster. Objects and the
//! player use the other camera y 0x03003CE0 = 0x110 and the background (BG2) only follows
//! the camera x / 4 (0x03003758). Then 0x081737A0[+0x45]:
//! * 1 (0x080A7C9C): walk right at 0x20; at camera x 0x80, jump (speed -0x60) -> 2.
//! * 2 (0x080A7CF0): walk on until the frame x's low byte is 0x36, then fly (gravity +4)
//!   until the speed is 0x0F or more -> 3, holding the anchor.
//! * 3 (0x080A7D4C): rise at -0x14 with the anchor; at BG camera y 0x70 the area switch
//!   0x03003CA0 = 3 (the airship, level 0x205) -> 4.
//!
//! The anchor (list type 0x09, init 0x0806EEC8: + (0xC, 8); main 0x0806EEE0) rises at -0x0C
//! while Mario hangs on it, and 0x080A8000 also takes 1 px off its y with each step of the
//! BG camera. Checked against mGBA frame by frame (positions, speeds, cameras, OAM).

use crate::obj::ObjSprite;
use crate::player::{Player, SUB};

/// List type of the anchor.
pub const ANCHOR: u16 = 0x09;
/// Header start kinds (byte 7 >> 5): the intro and the landing on the deck.
pub const INTRO_KIND: u8 = 6;
pub const LANDING_KIND: u8 = 7;
/// The deck (0x0809F43C): the frame x's low byte, y below the BG camera, speed.
pub const LANDING_X: i32 = 0x90;
pub const LANDING_DY: i32 = 0x80;
pub const LANDING_SPEED: i8 = -0x70;
/// The objects' and player's camera y during the intro (0x03003CE0, 0x080A7C9C).
pub const OBJECT_CAMERA_Y: i32 = 0x110;
/// The background's y during the intro: it does not follow the cameras.
// TODO(re): BG2's y during the intro was matched by eye against mGBA, not found in RAM.
pub const INTRO_BG_Y: i32 = 0xE4;
/// The BG camera y on arrival (0x0809F400).
const START_BG_Y: i32 = -5;
const WALK_SPEED: i8 = 0x20;
const JUMP_SPEED: i8 = -0x60;
const RISE_SPEED: i8 = -0x14;
const ANCHOR_RISE: i8 = -0x0C;
/// 0x080A7CF0: the jump starts at this frame x low byte.
const JUMP_AT: u8 = 0x36;
/// 0x080A7D4C: the switch to the airship at this BG camera y.
const SWITCH_Y: u8 = 0x6F;

/// What the intro did this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IntroStep {
    /// The area switch to the airship (0x03003CA0 = 3).
    pub switch: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Anchor {
    /// 1/256 px.
    x: i32,
    y: i32,
    vy: i8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intro {
    /// Player +0x45.
    pub state: u8,
    /// Camera x (0x03003750) and the BG camera y (0x03003754), px.
    pub camera_x: i32,
    pub bg_y: i32,
    /// 0x03003CED and player +0x3A.
    ced: u8,
    count: u8,
    /// The player's frame (+0 / +4, 1/256 px) and speed bytes.
    fx: i32,
    fy: i32,
    vx: i8,
    vy: i8,
    /// The anchor's list cell (px), and the anchor once spawned.
    anchor_at: Option<(i32, i32)>,
    anchor: Option<Anchor>,
}

impl Intro {
    /// Arriving with the player's frame at (fx, fy) px; `anchor` is the list's type-0x09
    /// cell (col, row).
    pub fn new(frame: (i32, i32), anchor: Option<(u16, u8)>) -> Self {
        Self {
            state: 1,
            camera_x: 0,
            bg_y: START_BG_Y,
            ced: 0,
            count: 0,
            fx: frame.0 * SUB,
            fy: frame.1 * SUB,
            vx: 0,
            vy: 0,
            anchor_at: anchor.map(|(c, r)| (c as i32 * 16, r as i32 * 16)),
            anchor: None,
        }
    }

    /// The level's rows are drawn this much lower than the objects' camera puts them
    /// (BG1 follows 0x03003754, the objects 0x03003CE0).
    pub fn level_shift(&self) -> i32 {
        OBJECT_CAMERA_Y - self.bg_y
    }

    /// One frame (0x08094780 -> 0x080A8000, then the anchor's main). `frame` is the frame
    /// counter (0x03003AE8) for the walk cycle.
    pub fn step(&mut self, player: &mut Player, frame: u32) -> IntroStep {
        let mut ev = IntroStep::default();
        self.ced = (self.ced & 0xF0).wrapping_add(0x70);
        // 0x080A8000.
        self.camera_x += 1;
        if self.camera_x > 0x5F {
            self.count = self.count.wrapping_add(1);
            if self.count as u16 + self.ced as u16 > 0xFF {
                self.bg_y += 1;
                if let Some(a) = &mut self.anchor {
                    a.y -= SUB;
                }
            }
        }
        let walk = (frame >> 1) & 3;
        match self.state {
            1 => {
                if (self.camera_x as u8 as i8) < 0 {
                    self.vy = JUMP_SPEED;
                    self.state = 2;
                }
                self.vx = WALK_SPEED;
                self.walk(player, walk);
            }
            2 => {
                if ((self.fx >> 8) as u8) < JUMP_AT {
                    self.walk(player, walk);
                } else {
                    self.move_y();
                    self.fx += self.vx as i32 * 16;
                    self.vy = self.vy.wrapping_add(4);
                    if self.vy.wrapping_sub(0x0F) >= 0 {
                        self.state = 3;
                    }
                    player.forced_pose = Some(crate::player::Pose::Jump);
                }
            }
            3 => {
                self.vy = RISE_SPEED;
                self.move_y();
                player.forced_pose = Some(crate::player::Pose::Hang);
                player.facing_left = true;
                if self.bg_y as u8 > SWITCH_Y {
                    ev.switch = true;
                    self.state = 4;
                }
            }
            _ => {}
        }
        self.place(player);
        self.step_anchor();
        ev
    }

    /// 0x080A855C walking: x on by the speed, the walk cycle by the frame counter.
    fn walk(&mut self, player: &mut Player, step: u32) {
        self.fx += self.vx as i32 * 16;
        player.forced_pose = Some(crate::player::Pose::Walk(step as u8));
        player.facing_left = false;
    }

    /// 0x080A3678: y on by the speed (falling at most 0x40).
    fn move_y(&mut self) {
        if self.vy >= 0x40 {
            self.vy = 0x40;
        }
        self.fy += self.vy as i32 * 16;
    }

    fn place(&self, player: &mut Player) {
        player.set_frame_sub(self.fx, self.fy);
        player.vx = self.vx as i32 * crate::player::UNIT;
        player.vy = self.vy as i32 * crate::player::UNIT;
        player.on_ground = self.state == 1;
    }

    /// The anchor: spawned when its column comes into the spawn window (camera + 0x110),
    /// then 0x0806EEE0.
    fn step_anchor(&mut self) {
        if let Some((x, y)) = self.anchor_at
            && (self.camera_x + 0x110) & !0xF >= x
        {
            self.anchor = Some(Anchor { x: (x + 0xC) * SUB, y: (y + 8) * SUB, vy: 0 });
            self.anchor_at = None;
        }
        if let Some(a) = &mut self.anchor
            && self.state == 3
        {
            a.vy = ANCHOR_RISE;
            a.y += a.vy as i32 * 16;
        }
    }

    /// The anchor as drawn (0x08047310 for its flukes, then 0x0806EEE0: the shaft and the
    /// chain up to the top of the screen).
    pub fn sprites(&self) -> Vec<ObjSprite> {
        let Some(a) = &self.anchor else { return Vec::new() };
        let sx = (a.x >> 8) - self.camera_x;
        let y = (a.y >> 8) - OBJECT_CAMERA_Y - 0xF;
        let tall = |x: i32, y: i32, tile: u16| ObjSprite { x, y, tile, shape: 2, size: 0, pal: 6, prio: 1, ..Default::default() };
        let mut out = Vec::new();
        if y + 0x10 >= -0x10 && y < 0xA0 {
            out.push(tall(sx + 8, y, 0x103));
            let mut cy = y - 0x10;
            while cy + 0x10 >= 0 {
                out.push(tall(sx + 8, cy, 0x105));
                cy -= 0x10;
            }
        }
        // TODO(re): 0x08047310's off-screen test for the flukes is approximated.
        if (-0x10..0xF0).contains(&sx) && (-0x10..0xA0).contains(&(y + 0x10)) {
            out.insert(0, tall(sx, y + 0x10, 0x102));
            out.insert(1, tall(sx + 8, y + 0x10, 0x104));
            out.insert(2, tall(sx + 0x10, y + 0x10, 0x102));
        }
        out
    }
}

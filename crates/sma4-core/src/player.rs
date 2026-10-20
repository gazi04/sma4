//! Player movement on the metatile grid, reimplemented from the ROM's player routines (see
//! docs/re-notes.md, "Player physics" and "Raccoon").
//!
//! Positions and velocities are integers in 1/256 pixel units, stepped once per frame. The
//! ROM keeps speeds as signed bytes in 1/16 px per frame; here a speed is that byte * 16.
//!
//! * Horizontal 0x08096884 -> 0x08096D24: target speed walk / run / P-run, acceleration
//!   table 0x08172C96 applied through a dithering accumulator (0x03003CED).
//! * Vertical 0x08096F9C: jump speed, gravity 1 (A held, rising fast) or 5, raccoon flutter,
//!   glide and flight; fall speed capped at 0x40 (0x080A3678).
//! * P-meter 0x0809F9A0 (0x03003C9F): a 7-bit shift register filled while running.
//! * Tail whip 0x080984E8 (raccoon): 18-frame spin; the tail hits on 4 of those frames.
//! * Slopes 0x0809A7E8 (tilesets with slopes): the floor comes from one probe under the
//!   middle of the feet and the shape's heightmap; walls are probed 8 px and more above
//!   the feet; walking uphill lowers the target speed.

use sma4_assets::TileKind;

use crate::Buttons;
use crate::level::{Level, TILE_PX};

/// Fixed-point scale: 1 pixel = 256 units.
pub const SUB: i32 = 256;
/// One ROM speed unit (1/16 px per frame) in 1/256 px.
const UNIT: i32 = 16;

/// Target speeds (1/16 px per frame): walk 0x08172C82, run (B) 0x08172C84, run with a full
/// P-meter 0x08172C86.
const WALK_MAX: i32 = 0x18;
/// Course-clear walk (0x080A8000 sets speed byte +0x1C = 0x14).
/// Grow / shrink animation length (+0x3B = 0x2F) and transformation puff (+0x3E = 0x17).
pub const MORPH_FRAMES: u8 = 0x2F;
pub const PUFF_FRAMES: u8 = 0x17;
const GOAL_WALK: i32 = 0x14;
const RUN_MAX: i32 = 0x28;
const P_MAX: i32 = 0x38;
/// Target speeds walking uphill: 0x08172C8A[10], with B 0x08172C8A[1] (0x08096884).
const UPHILL_WALK_MAX: i32 = 0x0D;
const UPHILL_RUN_MAX: i32 = 0x16;
/// Target speed with n Micro-Goombas stuck on the player: 0x08172C8A[n] (0x08096884),
/// for n = 1.. (six projectile slots at most).
const STUCK_MAX: [i32; 6] = [0x16, 0x15, 0x14, 0x13, 0x12, 0x11];
/// With slopes, the lowest wall probes sit 8 px above the feet (0x08172DB7: frame y +
/// 0x18), so the ground under a slope never blocks the feet.
const SLOPE_WALL_LIFT: i32 = 8;
/// Slope shapes that are the fill under a slope (0x0809A7E8: shapes 4 and 8): the floor
/// is looked up one tile higher.
const SLOPE_FILL: [u8; 2] = [4, 8];
/// Standing on nothing this deep into a tile, the floor is looked up one tile lower (so the
/// player follows a slope down); 0x0809A7E8.
const SLOPE_FOLLOW_DOWN: i32 = 10;
/// Acceleration table 0x08172C96 (1/256 of a speed unit per frame): friction (small 0xFF60,
/// other forms 0xFF20), acceleration 0xE0, skid 0x200.
const FRICTION_SMALL: i32 = -0xA0;
const FRICTION: i32 = -0xE0;
const ACCEL: i32 = 0xE0;
const SKID: i32 = 0x200;
/// Jump speed -0x38 - 0x0813DD3C[|speed| >> 4].
const JUMP_BASE: i32 = -0x38;
const JUMP_BONUS: [i32; 8] = [0, 2, 4, 8, 0x10, 0x18, 0x18, 0x18];
/// Gravity per frame: 1 while rising faster than -0x20 with A held, otherwise 5.
const GRAVITY_HOLD: i32 = 1;
const GRAVITY: i32 = 5;
const HOLD_ABOVE: i32 = -0x20;
const MAX_FALL: i32 = 0x40;
/// P-meter (0x0809F9A0): full at 0x7F; a bit shifts in every 8 frames while running, one
/// shifts out every 0x18 frames otherwise; a full meter is re-armed every 0x10 frames.
pub const P_FULL: u8 = 0x7F;
const P_FILL_FRAMES: u8 = 8;
const P_DRAIN_FRAMES: u8 = 0x18;
const P_HOLD_FRAMES: u8 = 0x10;
/// Raccoon (forms with 0x08173040 bit 0): A in the air sets a 16-frame flutter timer.
/// While it runs, a falling player glides at 0x10, a flying one rises at 0x18.
const FLUTTER_FRAMES: u8 = 0x10;
const GLIDE_FALL: i32 = 0x10;
const FLY_RISE: i32 = -0x18;
/// Flight time 0x80, counted down on odd frames (0x0809D72C / 0x0809F9A0).
const FLIGHT_TIME: u16 = 0x80;
/// Tail wag animation after A in the air (0x080981C4: +0x65 = 10).
const WAG_FRAMES: u8 = 10;
/// Tail whip (0x080984E8): +0x6A = 0x12, counted down every frame; the player turns
/// around at 11 and 3; the tail hits on 0x08169D88 frames 8, 9, 12, 13; blocks are probed
/// on frame 9 (0x0809A748).
const WHIP_FRAMES: u8 = 0x12;

/// Walk-cycle step length in frames by speed (0x0813DD54, indexed by |speed| >> 3 with
/// speed in 1/16 px per frame), from the ground animation routine 0x08097800.
const WALK_STEP_FRAMES: [u8; 16] = [7, 6, 5, 4, 3, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];
/// Step shown while standing still (also the standing pose).
pub const STAND_STEP: u8 = 2;

/// Collision box width in pixels.
pub const HITBOX_W: i32 = 12;
/// The ROM's player frame is 16x32 with the feet on its bottom row.
const FRAME_W: i32 = 16;
const FRAME_H: i32 = 32;

/// Power-up form (player byte +0x37 in the ROM: 0 small, 1 big, 3 raccoon).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Form {
    #[default]
    Small,
    Big,
    Raccoon,
}

impl Form {
    /// Box height: the ROM's head probes sit 16 px (small) / 6 px (others) below the top of
    /// a 32 px frame whose bottom is the feet, giving 16 and 26 px.
    pub const fn height(self) -> i32 {
        match self {
            Form::Small => 16,
            Form::Big | Form::Raccoon => 26,
        }
    }

    /// Hits the player can take: a hit turns raccoon into big and big into small
    /// (0x0804A5F4), and loses a life when small.
    pub const fn hit_points(self) -> u8 {
        match self {
            Form::Small => 1,
            Form::Big => 2,
            Form::Raccoon => 3,
        }
    }
}

/// What the sprite should show this frame; `sma4_assets::PLAYER_FRAMES` lists the cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pose {
    Walk(u8),
    /// Running at P-speed.
    Run(u8),
    Jump,
    Fall,
    /// Raccoon in flight: rising / falling, tail wag step 0-2.
    FlyUp(u8),
    FlyDown(u8),
    /// Raccoon falling with the tail wagging (glide), step 0-2.
    Glide(u8),
    /// Tail whip step 0-4 (0x6A >> 2), on the ground or in the air.
    Whip(u8),
    WhipAir(u8),
    /// Grow / shrink step 0-11: a cell of `sma4_assets::player_grow_path`.
    Morph(u8),
    /// Skidding: holding against the motion on the ground (0x08097800).
    Skid,
    /// Transformation smoke, tile 0x0817388C[i] (0x83, 0x82, 0x81, 0x80); the player is
    /// not drawn.
    Puff(u8),
}

impl Pose {
    /// Cell in the player sheet (`sma4_assets::PLAYER_*`).
    pub fn slot(self) -> usize {
        use sma4_assets as a;
        match self {
            Pose::Walk(s) => a::PLAYER_WALK + s as usize,
            Pose::Run(s) => a::PLAYER_RUN + s as usize,
            Pose::Jump => a::PLAYER_JUMP,
            Pose::Fall => a::PLAYER_FALL,
            Pose::FlyUp(s) => a::PLAYER_FLY_UP + s as usize,
            Pose::FlyDown(s) => a::PLAYER_FLY_DOWN + s as usize,
            Pose::Glide(s) => a::PLAYER_GLIDE + s as usize,
            Pose::Whip(s) => a::PLAYER_WHIP + s as usize,
            Pose::WhipAir(s) => a::PLAYER_WHIP_AIR + s as usize,
            Pose::Skid => a::PLAYER_SKID,
            // Drawn from their own sheets; the standing cell as a fallback.
            Pose::Morph(_) | Pose::Puff(_) => a::PLAYER_WALK + STAND_STEP as usize,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Player {
    /// Top-left of the hitbox, 1/256 px.
    pub x: i32,
    pub y: i32,
    pub vx: i32,
    pub vy: i32,
    pub on_ground: bool,
    pub facing_left: bool,
    pub form: Form,
    /// Walk-cycle step 0-3 (player byte +0x36); picks the ground pose.
    pub walk_step: u8,
    /// Frames into the current step (+0x4B).
    walk_timer: u8,
    /// P-meter (0x03003C9F) and its step timer (+0x69).
    pub p_meter: u8,
    p_timer: u8,
    /// Running at run speed this frame (+0x53) and allowed to take off (+0x1F).
    running: bool,
    fly_ready: bool,
    /// Flight time left (+0x20); counts down on odd frames.
    pub flight: u16,
    /// Raccoon flutter (+0x39) and tail wag animation (+0x65) timers.
    flutter: u8,
    wag: u8,
    /// Tail whip timer (+0x6A).
    pub whip: u8,
    /// Speed dithering accumulator (0x03003CED).
    dither: u8,
    /// Enemies stomped since last on the ground (+0x5A), for the score chain; cleared on
    /// landing (0x080A3A5C).
    pub stomp_chain: u16,
    /// Grow / shrink animation timer (+0x3B): 0x2F, -1 per frame; the player is frozen.
    pub morph: u8,
    /// Smoke puff of a transformation (+0x3E): 0x17, -1 per frame; the player is hidden
    /// and frozen.
    pub puff: u8,
    /// Standing on a slope that pushes against the motion (0x03003CE4).
    uphill: bool,
    /// Micro-Goombas riding the player (0x03007820 + 7), set by the game every frame.
    pub stuck: u8,
    /// Skidding this frame: the skid pose, and the skid dust follows (0x08097800).
    pub skidding: bool,
}

impl Player {
    /// `px, py` = top-left of a small player's box.
    pub fn spawn(px: i32, py: i32) -> Self {
        Self { x: px * SUB, y: py * SUB, ..Self::default() }
    }

    pub fn px(&self) -> (i32, i32) {
        (self.x.div_euclid(SUB), self.y.div_euclid(SUB))
    }

    pub fn height(&self) -> i32 {
        self.form.height()
    }

    /// Centre of the box in pixels.
    pub fn centre(&self) -> (i32, i32) {
        let (px, py) = self.px();
        (px + HITBOX_W / 2, py + self.height() / 2)
    }

    /// Top-left of the ROM's 16x32 player frame, in pixels.
    pub fn frame(&self) -> (i32, i32) {
        let (px, py) = self.px();
        (px + HITBOX_W / 2 - FRAME_W / 2, py + self.height() - FRAME_H)
    }

    fn is_raccoon(&self) -> bool {
        self.form == Form::Raccoon
    }

    /// Changes form, keeping the feet where they are.
    pub fn set_form(&mut self, form: Form) {
        self.y += (self.form.height() - form.height()) * SUB;
        self.form = form;
        if form != Form::Raccoon {
            self.flight = 0;
            self.whip = 0;
        }
    }

    /// Mushroom (0x0806B158): small -> big, with the grow animation (+0x3B = 0x2F; the form
    /// request +0x50 is applied the next frame, so the animation runs with the new form).
    pub fn grow(&mut self) {
        if self.form == Form::Small {
            self.set_form(Form::Big);
            self.morph = MORPH_FRAMES;
        }
    }

    /// Super Leaf (0x0806B254): any other form -> raccoon behind a puff of smoke (+0x3E =
    /// 0x17, sound 0x4B).
    pub fn leaf(&mut self) {
        if self.form != Form::Raccoon {
            self.set_form(Form::Raccoon);
            self.puff = PUFF_FRAMES;
        }
    }

    /// Busy with a transformation: frozen, and hits are ignored (0x0804A5F4 returns early
    /// while +0x3B or +0x3E is set).
    pub fn transforming(&self) -> bool {
        self.morph != 0 || self.puff != 0
    }

    /// Takes a hit. Returns false if the player had no hit points left.
    pub fn hurt(&mut self) -> bool {
        match self.form {
            Form::Raccoon => self.set_form(Form::Big),
            Form::Big => {
                // 0x0804A5F4: big shrinks with the same animation, played backwards.
                self.set_form(Form::Small);
                self.morph = MORPH_FRAMES;
            }
            Form::Small => return false,
        }
        true
    }

    /// Advances one frame. `frame` is the global frame counter. Returns the tile the
    /// head-centre probe hit while moving up, if any.
    pub fn step(&mut self, held: Buttons, pressed: Buttons, map: &Level, frame: u32) -> Option<(i32, i32)> {
        // 0x080A8000: the puff (+0x3E) and the grow / shrink steps (+0x3B) replace the
        // player's update while they run.
        self.skidding = false;
        if self.puff != 0 {
            self.puff -= 1;
            return None;
        }
        if self.morph != 0 {
            self.morph -= 1;
            return None;
        }
        // Per-frame timers of the player loop (0x08094780: bytes +0x64..+0x70).
        self.p_timer = self.p_timer.saturating_sub(1);
        self.wag = self.wag.saturating_sub(1);
        self.whip = self.whip.saturating_sub(1);
        self.dither = (self.dither & 0xF0).wrapping_add(0x70);

        self.horizontal(held);
        self.vertical(held, pressed);
        if self.is_raccoon() {
            self.raccoon(pressed);
        }
        self.update_p_meter(frame);
        self.move_x(map);
        let head_hit = self.move_y(map);
        if self.on_ground {
            self.animate_walk(held);
            self.skidding = self.skids(held);
        }
        head_hit
    }

    /// Adds `delta` (1/256 speed units) to the speed byte through the dithering accumulator.
    fn accelerate(&mut self, delta: i32) {
        let step = (self.dither as i32 + delta).div_euclid(256);
        self.vx += step * UNIT;
    }

    /// 0x08096884 / 0x08096D24.
    fn horizontal(&mut self, held: Buttons) {
        let dir = match (held.contains(Buttons::LEFT), held.contains(Buttons::RIGHT)) {
            (true, false) => -1,
            (false, true) => 1,
            _ => 0,
        };
        let speed = self.vx / UNIT;
        let run = held.contains(Buttons::B);
        let target = match (run, self.p_meter == P_FULL) {
            _ if self.uphill => {
                // The walk cycle also runs twice as fast (+0x4B).
                self.walk_timer += 1;
                if run { UPHILL_RUN_MAX } else { UPHILL_WALK_MAX }
            }
            _ if self.stuck > 0 => STUCK_MAX[(self.stuck as usize).min(STUCK_MAX.len()) - 1],
            (false, _) => WALK_MAX,
            (true, false) => RUN_MAX,
            (true, true) => P_MAX,
        };
        // P-meter input (+0x53) and take-off permission (+0x1F); not uphill, nor with
        // Micro-Goombas on.
        if run && self.on_ground && !self.uphill && self.stuck == 0 {
            self.running = speed.abs() >= RUN_MAX;
            if speed.abs() >= WALK_MAX && dir != 0 && dir == speed.signum() && self.p_timer == 0 && self.p_meter > 0x3E {
                self.fly_ready = true;
            }
        }
        let friction = if self.form == Form::Small { FRICTION_SMALL } else { FRICTION };
        if dir == 0 {
            if self.on_ground && speed != 0 {
                self.accelerate(if speed < 0 { -friction } else { friction });
            }
            return;
        }
        if self.whip == 0 {
            self.facing_left = dir < 0;
        }
        let delta = if speed != 0 && speed.signum() != dir {
            SKID
        } else if speed.abs() == target {
            return;
        } else if speed.abs() < target {
            ACCEL
        } else if !self.on_ground {
            return;
        } else {
            friction
        };
        self.accelerate(delta * dir);
    }

    /// 0x08096F9C: jump, gravity.
    fn vertical(&mut self, held: Buttons, pressed: Buttons) {
        let mut vy = self.vy / UNIT;
        if self.on_ground && pressed.contains(Buttons::A) {
            let speed = (self.vx / UNIT).unsigned_abs() as usize;
            vy = JUMP_BASE - JUMP_BONUS[(speed >> 4).min(7)];
            self.on_ground = false;
            self.flutter = 0;
            // 0x0809D72C: a full P-meter and a running start lets the raccoon take off.
            if self.p_meter == P_FULL && self.fly_ready {
                self.fly_ready = false;
                self.flight = FLIGHT_TIME;
            }
        }
        if self.on_ground {
            if !self.is_raccoon() {
                self.flight = 0;
            }
            self.vy = vy * UNIT;
            return;
        }
        let g = if vy < HOLD_ABOVE && held.contains(Buttons::A) { GRAVITY_HOLD } else { GRAVITY };
        vy += g;
        self.flutter = self.flutter.saturating_sub(1);
        if self.is_raccoon() && pressed.contains(Buttons::A) {
            self.flutter = FLUTTER_FRAMES;
        }
        if self.flutter != 0 && vy >= FLY_RISE {
            if self.flight == 0 {
                if vy >= GLIDE_FALL {
                    vy = GLIDE_FALL;
                }
            } else if self.flight < 0xF {
                vy = if self.flight & 8 != 0 { -0x10 } else { 0 };
            } else {
                vy = FLY_RISE;
            }
        }
        self.vy = vy.min(MAX_FALL) * UNIT;
    }

    /// Raccoon extras: tail wag (0x080981C4), speed limit while flying or gliding, tail
    /// whip (0x080984E8).
    fn raccoon(&mut self, pressed: Buttons) {
        if !self.on_ground {
            if pressed.contains(Buttons::A) {
                self.wag = WAG_FRAMES;
            }
            // While flying or fluttering, speed above walking speed bleeds off by 1.
            let speed = self.vx / UNIT;
            if (self.flight != 0 || self.flutter != 0) && speed.abs() > WALK_MAX {
                self.vx -= speed.signum() * UNIT;
            }
        }
        if pressed.contains(Buttons::B) && self.whip == 0 {
            self.whip = WHIP_FRAMES;
        }
        if self.whip == 11 || self.whip == 3 {
            self.facing_left = !self.facing_left;
        }
    }

    /// 0x0809F9A0.
    fn update_p_meter(&mut self, frame: u32) {
        if self.flight != 0 {
            if frame & 1 != 0 {
                self.flight -= 1;
                if self.flight == 0 {
                    self.p_meter = 0;
                }
            }
        } else if self.p_timer == 0 {
            if self.p_meter == P_FULL && self.running {
                self.p_timer = P_HOLD_FRAMES;
            } else if self.running {
                self.p_meter = (self.p_meter << 1 | 1) & P_FULL;
                self.p_timer = P_FILL_FRAMES;
            } else {
                self.p_meter >>= 1;
                self.fly_ready = false;
                if self.p_meter != 0 {
                    self.p_timer = P_DRAIN_FRAMES;
                }
            }
        }
        self.running = false;
    }

    /// Tail hit box (x, y, w, h) in pixels while the whip is on a hitting frame
    /// (0x08049970: x + 0x08169D82[facing], 12 wide; y + 16, 15 high in the 16x32 frame).
    pub fn tail_box(&self) -> Option<(i32, i32, i32, i32)> {
        if !self.is_raccoon() || !matches!(self.whip, 8 | 9 | 12 | 13) {
            return None;
        }
        let (fx, fy) = self.frame();
        let dx = if self.facing_left { 17 } else { -12 };
        Some((fx + dx, fy + 16, 12, 15))
    }

    /// Block cell the tail probes this frame (0x0809A748 on whip frame 9: (dy, dx) from
    /// 0x08172DF8 = (24, -7) facing right, (24, 22) facing left).
    pub fn tail_block(&self) -> Option<(i32, i32)> {
        if !self.is_raccoon() || self.whip != 9 {
            return None;
        }
        let (fx, fy) = self.frame();
        let dx = if self.facing_left { 22 } else { -7 };
        Some(((fx + dx).div_euclid(TILE_PX), (fy + 24).div_euclid(TILE_PX)))
    }

    /// Sprite pose, following the ROM's pose routines (0x08097800, 0x08097ACC,
    /// 0x080981C4, 0x080984E8, 0x080983F8).
    pub fn pose(&self) -> Pose {
        if self.puff != 0 {
            return Pose::Puff((self.puff & 0xC) >> 2);
        }
        if self.morph != 0 {
            // 0x080A8000: step morph >> 2 of 0x0813DE2F, reversed when the form is small.
            let step = self.morph >> 2;
            return Pose::Morph(if self.form == Form::Small { 11 - step } else { step });
        }
        if self.whip != 0 && self.is_raccoon() {
            let step = (self.whip >> 2).min(4);
            return if self.on_ground { Pose::Whip(step) } else { Pose::WhipAir(step) };
        }
        if self.on_ground {
            if self.skidding {
                return Pose::Skid;
            }
            let speed = (self.vx / UNIT).abs();
            return if speed >= P_MAX - 1 { Pose::Run(self.walk_step) } else { Pose::Walk(self.walk_step) };
        }
        let falling = self.vy >= 0;
        if self.is_raccoon() {
            let wag = (self.wag >> 2).min(2);
            return match (self.flight != 0, falling) {
                (true, false) => Pose::FlyUp(wag),
                (true, true) => Pose::FlyDown(wag),
                (false, false) => Pose::Jump,
                (false, true) => Pose::Glide(wag),
            };
        }
        if falling && self.form == Form::Big { Pose::Fall } else { Pose::Jump }
    }

    /// After the goal card is touched (0x080A8000 while +0x43 is set; the goal object sets it
    /// every frame the player is on the ground, 0x08067E70 / 0x0806743C): no input. On the
    /// ground the player walks right at speed byte 0x14 (0x080A855C), the walk cycle stepped
    /// by the frame counter ((frame & 6) >> 1, 0x080A85CC); in the air the goal object clears
    /// the speed and the player just falls. `stop` freezes it (+0x40 = 0xFF once it is 0x120
    /// px into the screen).
    pub fn goal_walk(&mut self, map: &Level, frame: u32, stop: bool) {
        self.skidding = false;
        self.whip = 0;
        self.flight = 0;
        self.flutter = 0;
        if stop {
            self.vx = 0;
            return;
        }
        if self.on_ground {
            self.vx = GOAL_WALK * UNIT;
            self.facing_left = false;
            self.walk_step = ((frame & 6) >> 1) as u8;
        } else {
            self.vx = 0;
        }
        self.vertical(Buttons::empty(), Buttons::empty());
        // The ROM lets the player walk out past the end of the level; here the map's edge
        // is a wall, so it is ignored once reached.
        if self.px().0 + HITBOX_W + 1 >= map.pixel_width() {
            self.x += self.vx;
        } else {
            self.move_x(map);
        }
        self.move_y(map);
    }

    /// 0x08097800: on the ground, moving (speed byte not -1..1) and holding the other way
    /// (+0x63 = the direction of the motion, compared shifted into the pad's left / right
    /// bits).
    fn skids(&self, held: Buttons) -> bool {
        let speed = self.vx / UNIT;
        speed.abs() > 1 && held.contains(if speed > 0 { Buttons::LEFT } else { Buttons::RIGHT })
    }

    /// 0x08097800: advance the walk cycle at a speed-dependent rate; standing still with
    /// no direction held shows the standing step.
    fn animate_walk(&mut self, held: Buttons) {
        let speed = (self.vx.abs() / UNIT) as usize;
        self.walk_timer += 1;
        if self.walk_timer >= WALK_STEP_FRAMES[(speed >> 3).min(15)] {
            self.walk_timer = 0;
            self.walk_step = (self.walk_step + 1) & 3;
        }
        if !held.contains(Buttons::LEFT) && !held.contains(Buttons::RIGHT) && speed == 0 {
            self.walk_step = STAND_STEP;
        }
    }

    fn move_x(&mut self, map: &Level) {
        self.x += self.vx;
        let (px, py) = self.px();
        let lift = if map.slopes.is_some() { SLOPE_WALL_LIFT } else { 0 };
        let rows = tile_span(py, self.height() - lift);
        if self.vx > 0 {
            let tx = (px + HITBOX_W - 1).div_euclid(TILE_PX);
            if rows.clone().any(|ty| map.at(tx, ty) == TileKind::Solid) {
                self.x = (tx * TILE_PX - HITBOX_W) * SUB;
                self.vx = 0;
            }
        } else if self.vx < 0 {
            let tx = px.div_euclid(TILE_PX);
            if rows.clone().any(|ty| map.at(tx, ty) == TileKind::Solid) {
                self.x = (tx + 1) * TILE_PX * SUB;
                self.vx = 0;
            }
        }
    }

    fn move_y(&mut self, map: &Level) -> Option<(i32, i32)> {
        let h = self.height();
        let old_bottom = self.y.div_euclid(SUB) + h;
        self.y += self.vy;
        let (px, py) = self.px();
        let cols = tile_span(px, HITBOX_W);
        let grounded = self.on_ground;
        self.on_ground = false;
        self.uphill = false;
        if self.vy >= 0 && map.slopes.is_some() {
            self.land_on_slope(map, grounded);
            return None;
        }
        if self.vy >= 0 {
            let ty = (py + h).div_euclid(TILE_PX);
            let top = ty * TILE_PX;
            // "Top" tiles only catch the player if they were above the tile last frame.
            let lands = cols.clone().any(|tx| match map.at(tx, ty) {
                TileKind::Solid => true,
                TileKind::Top => old_bottom <= top,
                TileKind::Empty => false,
            });
            if lands && py + h >= top {
                self.land(top);
            }
            None
        } else {
            let ty = py.div_euclid(TILE_PX);
            if cols.clone().any(|tx| map.at(tx, ty) == TileKind::Solid) {
                self.y = (ty + 1) * TILE_PX * SUB;
                self.vy = 0;
                // The ROM's head-centre probe (probe 1) decides which block was hit.
                let centre_tx = (px + HITBOX_W / 2).div_euclid(TILE_PX);
                return (map.at(centre_tx, ty) == TileKind::Solid).then_some((centre_tx, ty));
            }
            None
        }
    }

    /// Puts the feet on `floor` (px) and lands (0x080A3A5C).
    fn land(&mut self, floor: i32) {
        self.y = (floor - self.height()) * SUB;
        self.vy = 0;
        self.on_ground = true;
        self.flutter = 0;
        self.stomp_chain = 0;
    }

    /// 0x0809A7E8: the floor of a slope tileset, from the probe under the middle of the
    /// feet (frame x + 8, y + 0x20).
    fn land_on_slope(&mut self, map: &Level, grounded: bool) {
        let (px, py) = self.px();
        let bottom = py + self.height();
        let cx = px + HITBOX_W / 2;
        let tx = cx.div_euclid(TILE_PX);
        let mut ty = bottom.div_euclid(TILE_PX);
        let mut shape = map.slope(tx, ty);
        if SLOPE_FILL.contains(&shape) {
            // The fill under a slope: the slope itself is one tile up.
            let above = map.slope(tx, ty - 1);
            if map.slope_floor(above, cx).is_some() {
                shape = above;
                ty -= 1;
            }
        } else if shape == 0 && grounded && bottom.rem_euclid(TILE_PX) >= SLOPE_FOLLOW_DOWN {
            let below = map.slope(tx, ty + 1);
            if map.slope_floor(below, cx).is_some() {
                shape = below;
                ty += 1;
            }
        }
        let Some(offset) = map.slope_floor(shape, cx) else { return };
        let floor = ty * TILE_PX + offset;
        // On the ground the feet follow the floor up and down; in the air they only land
        // once they reach it.
        if !grounded && bottom < floor {
            return;
        }
        self.land(floor);
        let push = map.slope_push(shape);
        self.uphill = push != 0 && (push < 0) != (self.vx < 0);
    }
}

/// Tile indices covered by `[start, start + len)` pixels.
fn tile_span(start: i32, len: i32) -> std::ops::RangeInclusive<i32> {
    start.div_euclid(TILE_PX)..=(start + len - 1).div_euclid(TILE_PX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 40x6 map with a solid floor on the last row.
    fn floor() -> Level {
        let (w, h) = (40, 6);
        let mut kinds = vec![TileKind::Empty; w * h];
        for x in 0..w {
            kinds[(h - 1) * w + x] = TileKind::Solid;
        }
        Level::from_kinds(w, h, &kinds)
    }

    struct Sim {
        p: Player,
        map: Level,
        frame: u32,
        held: Buttons,
    }

    impl Sim {
        fn new(map: Level, x: i32) -> Self {
            let mut s = Self { p: Player::spawn(x, 0), map, frame: 0, held: Buttons::empty() };
            s.run(Buttons::empty(), 120);
            s
        }

        fn run(&mut self, held: Buttons, frames: u32) -> Option<(i32, i32)> {
            let mut hit = None;
            for _ in 0..frames {
                let pressed = held & !self.held;
                self.held = held;
                self.frame += 1;
                hit = hit.or(self.p.step(held, pressed, &self.map, self.frame));
            }
            hit
        }
    }

    #[test]
    fn falls_and_lands_on_floor() {
        let s = Sim::new(floor(), 32);
        assert!(s.p.on_ground);
        assert_eq!(s.p.px().1, 5 * TILE_PX - 16);
    }

    #[test]
    fn walks_and_runs_at_the_rom_speeds() {
        let mut s = Sim::new(floor(), 32);
        s.run(Buttons::RIGHT, 40);
        assert_eq!(s.p.vx, WALK_MAX * UNIT);
        s.run(Buttons::RIGHT | Buttons::B, 30);
        assert_eq!(s.p.vx, RUN_MAX * UNIT);
    }

    #[test]
    fn running_fills_the_p_meter_and_unlocks_p_speed() {
        let mut s = Sim::new(floor(), 16);
        s.run(Buttons::RIGHT | Buttons::B, 120);
        assert_eq!(s.p.p_meter, P_FULL);
        s.run(Buttons::RIGHT | Buttons::B, 30);
        assert_eq!(s.p.vx, P_MAX * UNIT);
        // Letting go drains it again.
        s.run(Buttons::empty(), 7 * P_DRAIN_FRAMES as u32 + 2);
        assert_eq!(s.p.p_meter, 0);
    }

    #[test]
    fn walk_cycle_speeds_up_and_stops_on_the_standing_step() {
        let mut s = Sim::new(floor(), 32);
        assert_eq!(s.p.walk_step, STAND_STEP);
        s.run(Buttons::RIGHT, 60);
        // Walk speed 0x18: a new step every 0x0813DD54[3] = 4 frames.
        let s0 = s.p.walk_step;
        s.run(Buttons::RIGHT, 4);
        assert_eq!(s.p.walk_step, (s0 + 1) & 3);
        s.run(Buttons::empty(), 60);
        assert_eq!(s.p.walk_step, STAND_STEP);
    }

    #[test]
    fn jump_height_depends_on_holding_a() {
        let mut s = Sim::new(floor(), 32);
        let ground = s.p.px().1;
        let peak = |s: &mut Sim, held: Buttons| {
            let mut top = ground;
            s.run(Buttons::empty(), 2);
            s.run(Buttons::A, 1);
            for _ in 0..80 {
                s.run(held, 1);
                top = top.min(s.p.px().1);
            }
            ground - top
        };
        let tap = peak(&mut s, Buttons::empty());
        let hold = peak(&mut s, Buttons::A);
        assert!(hold > tap * 2, "hold {hold} tap {tap}");
        assert!((60..=72).contains(&hold), "standing jump with A held: {hold} px");
    }

    #[test]
    fn wall_stops_player() {
        let mut s = Sim::new(floor(), 32);
        s.run(Buttons::LEFT, 300);
        assert_eq!(s.p.px().0, 0);
        assert_eq!(s.p.vx, 0);
    }

    #[test]
    fn top_tile_is_passable_from_below() {
        let (w, h) = (10, 6);
        let mut kinds = vec![TileKind::Empty; w * h];
        for x in 0..w {
            kinds[(h - 1) * w + x] = TileKind::Solid;
            kinds[3 * w + x] = TileKind::Top;
        }
        let mut s = Sim::new(Level::from_kinds(w, h, &kinds), 32);
        s.p.y = (4 * TILE_PX - 16) * SUB;
        s.run(Buttons::empty(), 60);
        s.run(Buttons::A, 30);
        s.run(Buttons::empty(), 60);
        assert_eq!(s.p.px().1, 3 * TILE_PX - 16);
    }

    #[test]
    fn head_hit_reports_centre_tile() {
        let block = (2, 2);
        let mut s = Sim::new(floor(), block.0 * TILE_PX + 2);
        s.map.set(block.0, block.1, 0x53);
        let hit = s.run(Buttons::A, 40);
        assert_eq!(hit, Some(block));
    }

    #[test]
    fn hits_step_down_through_the_forms() {
        let mut s = Sim::new(floor(), 32);
        let feet = s.p.px().1 + s.p.height();
        s.p.leaf();
        assert_eq!((s.p.form, s.p.form.hit_points()), (Form::Raccoon, 3));
        assert_eq!(s.p.px().1 + s.p.height(), feet);
        assert!(s.p.hurt());
        assert_eq!(s.p.form, Form::Big);
        assert!(s.p.hurt());
        assert_eq!(s.p.form, Form::Small);
        assert_eq!(s.p.px().1 + s.p.height(), feet);
        assert!(!s.p.hurt());
    }

    #[test]
    fn raccoon_glides_when_tapping_a() {
        // Tall empty map: fall for a while.
        let (w, h) = (10, 60);
        let mut kinds = vec![TileKind::Empty; w * h];
        kinds[(h - 1) * w..].fill(TileKind::Solid);
        let mut s = Sim::new(Level::from_kinds(w, h, &kinds), 32);
        s.p.leaf();
        s.p.y = 0;
        s.p.on_ground = false;
        s.run(Buttons::empty(), 20);
        assert_eq!(s.p.vy, MAX_FALL * UNIT, "plain fall");
        s.p.y = 0;
        for _ in 0..4 {
            s.run(Buttons::A, 1);
            s.run(Buttons::empty(), 7);
        }
        assert_eq!(s.p.vy, GLIDE_FALL * UNIT, "tapping A slows the fall");
        assert!(matches!(s.p.pose(), Pose::Glide(_)));
    }

    #[test]
    fn raccoon_flies_after_a_running_start() {
        let mut s = Sim::new(floor(), 16);
        s.p.leaf();
        s.run(Buttons::RIGHT | Buttons::B, 140);
        assert_eq!(s.p.p_meter, P_FULL);
        let ground = s.p.px().1;
        s.run(Buttons::RIGHT | Buttons::B | Buttons::A, 1);
        // Counted down on odd frames, possibly already on the take-off frame.
        assert!(s.p.flight >= FLIGHT_TIME - 1);
        // Keep tapping A: the raccoon climbs.
        for _ in 0..20 {
            s.run(Buttons::RIGHT | Buttons::B, 3);
            s.run(Buttons::RIGHT | Buttons::B | Buttons::A, 3);
        }
        assert!(ground - s.p.px().1 > 80, "climbed {} px", ground - s.p.px().1);
        assert!(matches!(s.p.pose(), Pose::FlyUp(_) | Pose::FlyDown(_)));
        // Flight time runs out after 0x80 odd frames; the P-meter empties.
        s.run(Buttons::empty(), 2 * FLIGHT_TIME as u32);
        assert_eq!((s.p.flight, s.p.p_meter), (0, 0));
    }

    #[test]
    fn small_player_cannot_fly() {
        let mut s = Sim::new(floor(), 16);
        s.run(Buttons::RIGHT | Buttons::B, 140);
        s.run(Buttons::RIGHT | Buttons::B | Buttons::A, 1);
        assert!(s.p.flight >= FLIGHT_TIME - 1, "the timer starts for any form...");
        s.run(Buttons::RIGHT | Buttons::B, 1);
        s.run(Buttons::empty(), 90);
        assert!(s.p.on_ground, "...but only the raccoon uses it");
    }

    #[test]
    fn mushroom_grows_in_12_steps_and_leaf_puffs_into_a_raccoon() {
        let mut s = Sim::new(floor(), 64);
        s.p.grow();
        assert_eq!(s.p.form, Form::Big);
        let (x, y) = (s.p.x, s.p.y);
        let mut steps = Vec::new();
        while s.p.transforming() {
            if let Pose::Morph(i) = s.p.pose() {
                if steps.last() != Some(&i) {
                    steps.push(i);
                }
            }
            s.run(Buttons::RIGHT | Buttons::A, 1);
        }
        assert_eq!(steps, (0..12).rev().collect::<Vec<u8>>(), "growing plays 0x0813DE2F backwards");
        assert_eq!((s.p.x, s.p.y), (x, y), "frozen meanwhile");
        // A hit plays it forwards while small.
        assert!(s.p.hurt());
        assert_eq!((s.p.form, s.p.pose()), (Form::Small, Pose::Morph(0)));
        s.run(Buttons::empty(), MORPH_FRAMES as u32);
        assert!(!s.p.transforming());
        s.p.leaf();
        assert_eq!((s.p.form, s.p.pose()), (Form::Raccoon, Pose::Puff(1)));
        let mut frames = 0;
        while s.p.transforming() {
            s.run(Buttons::empty(), 1);
            frames += 1;
        }
        assert_eq!(frames, PUFF_FRAMES as u32);
    }

    #[test]
    fn tail_whip_hits_on_four_frames_and_turns_around() {
        let mut s = Sim::new(floor(), 64);
        s.p.leaf();
        s.run(Buttons::empty(), PUFF_FRAMES as u32);
        s.p.facing_left = false;
        let mut boxes = 0;
        let mut blocks = 0;
        s.run(Buttons::B, 1);
        for _ in 0..WHIP_FRAMES {
            boxes += s.p.tail_box().is_some() as u32;
            blocks += s.p.tail_block().is_some() as u32;
            s.run(Buttons::B, 1);
        }
        assert_eq!((boxes, blocks), (4, 1));
        assert!(!s.p.facing_left, "turned twice, facing the same way again");
    }

    /// 24x8 hills level (tileset 3 thresholds): flat ground on row 7, a 45-degree hill up
    /// from column 6 to a flat top on row 4 (columns 9-12) and back down to column 15.
    fn hill() -> Level {
        use sma4_assets::{BlockRules, Slopes, Solidity};
        const UP: u16 = 0x99;
        const FLAT: u16 = 0x9A;
        const DOWN: u16 = 0x9B;
        const FILL: u16 = 0xAB;
        const SKY: u16 = 0x80;
        let (w, h) = (24, 8);
        let mut m = vec![SKY; w * h];
        let mut put = |x: usize, y: usize, t: u16| m[y * w + x] = t;
        for x in 0..w {
            put(x, 7, FLAT);
        }
        for (x, top, t) in [(6, 6, UP), (7, 5, UP), (8, 4, UP), (13, 4, DOWN), (14, 5, DOWN), (15, 6, DOWN)] {
            put(x, top, t);
            (top + 1..h).for_each(|y| put(x, y, FILL));
        }
        for x in 9..13 {
            put(x, 4, FLAT);
            (5..h).for_each(|y| put(x, y, FILL));
        }
        // Shapes 1 (up), 2 (down), 4 (fill), 7 (flat) as in the ROM tables.
        let mut slopes = Slopes::default();
        for (id, shape) in [(UP, 1), (FLAT, 7), (DOWN, 2), (FILL, 4)] {
            slopes.shape[id as usize] = shape;
        }
        slopes.height[1] = std::array::from_fn(|x| 15 - x as u8);
        slopes.height[2] = std::array::from_fn(|x| x as u8);
        for k in [1, 2, 4, 7] {
            slopes.no_floor[k] = false;
        }
        slopes.height[4] = [0; 16];
        slopes.height[7] = [0; 16];
        slopes.push[1] = -3;
        slopes.push[2] = 3;
        let solidity =
            Solidity { top: [0x25, 0x5F, 0x99, 0xE2], solid: [0x2E, 0x5F, 0xA6, 0xF0], high: [0; sma4_assets::HIGH_IDS] };
        Level::new(w, h, m, solidity, BlockRules::default()).with_slopes(Some(slopes))
    }

    #[test]
    fn walks_over_a_hill_without_jumping() {
        let mut s = Sim::new(hill(), 8);
        assert!(s.p.on_ground);
        let mut last_up = 0;
        for _ in 0..240 {
            s.run(Buttons::RIGHT, 1);
            assert!(s.p.on_ground, "left the ground at x {}", s.p.px().0);
            let cx = s.p.px().0 + HITBOX_W / 2;
            let (tx, bottom) = (cx / TILE_PX, s.p.px().1 + s.p.height());
            // The feet stay on the heightmap: 45 degrees up, flat on top, 45 degrees down.
            let floor = match tx {
                6..=8 => (13 - tx) * TILE_PX - 1 - cx % TILE_PX,
                9..=12 => 4 * TILE_PX,
                13..=15 => (tx - 9) * TILE_PX + cx % TILE_PX,
                _ => 7 * TILE_PX,
            };
            assert_eq!(bottom, floor, "feet off the slope at x {cx}");
            if (6..=8).contains(&tx) {
                last_up = s.p.vx / UNIT;
            }
        }
        assert!(s.p.px().0 > 17 * TILE_PX, "got over the hill");
        assert!(last_up < WALK_MAX, "slowed down uphill: {last_up:#x}");
    }

    #[test]
    fn walking_uphill_is_slower() {
        let mut s = Sim::new(hill(), 6 * TILE_PX + 4);
        let mut max = 0;
        for _ in 0..20 {
            s.run(Buttons::RIGHT, 1);
            max = max.max(s.p.vx / UNIT);
        }
        assert_eq!(max, UPHILL_WALK_MAX);
        // Downhill (left) the walk speed is the usual one.
        s.run(Buttons::LEFT, 40);
        assert_eq!(s.p.vx / UNIT, -WALK_MAX);
    }

    #[test]
    fn lands_on_a_slope_from_a_jump() {
        let mut s = Sim::new(hill(), 7 * TILE_PX);
        assert!(s.p.on_ground);
        let cx = s.p.px().0 + HITBOX_W / 2;
        assert_eq!(s.p.px().1 + s.p.height(), 5 * TILE_PX + 15 - cx % TILE_PX);
        s.run(Buttons::A, 1);
        assert!(!s.p.on_ground);
        s.run(Buttons::empty(), 80);
        assert!(s.p.on_ground);
        assert_eq!(s.p.px().1 + s.p.height(), 5 * TILE_PX + 15 - cx % TILE_PX, "back on the slope");
    }

    #[test]
    fn turning_around_at_speed_skids_until_the_motion_reverses() {
        let mut s = Sim::new(floor(), 16);
        s.run(Buttons::RIGHT | Buttons::B, 40);
        assert!(!s.p.skidding);
        let v0 = s.p.vx / UNIT;
        let mut skid = 0;
        for _ in 0..40 {
            s.run(Buttons::LEFT, 1);
            if s.p.skidding {
                skid += 1;
                assert_eq!(s.p.pose(), Pose::Skid);
                assert!(s.p.vx > 0 && s.p.facing_left, "slides on, facing the new way");
            }
        }
        // Down to 1 at the skid deceleration (0x200 / 256 = 2 per frame).
        assert_eq!(skid, (v0 - 2) / 2, "skid frames from speed {v0:#x}");
        assert!(!s.p.skidding && s.p.vx < 0);
        // Letting go is no skid, nor is a jump.
        s.run(Buttons::empty(), 1);
        assert!(!s.p.skidding);
    }
}

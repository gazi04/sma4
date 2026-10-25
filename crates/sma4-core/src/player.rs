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
pub const UNIT: i32 = 16;

/// Target speeds (1/16 px per frame): walk 0x08172C82, run (B) 0x08172C84, run with a full
/// P-meter 0x08172C86.
const WALK_MAX: i32 = 0x18;
/// Course-clear walk (0x080A8000 sets speed byte +0x1C = 0x14).
/// Grow / shrink animation length (+0x3B = 0x2F) and transformation puff (+0x3E = 0x17).
pub const MORPH_FRAMES: u8 = 0x2F;
pub const PUFF_FRAMES: u8 = 0x17;
/// Fire Flower flash (+0x3F) and the star (+0x3D, halved: 0xE0 counts on odd frames).
pub const FIRE_FLASH: u8 = 0x1F;
pub const STAR_TIME: u8 = 0xE0;
/// Throw pose timer (+0x67, 0x0808BECC).
pub const THROW_TIME: u8 = 0xB;
/// White block (0x0809E164): its top tiles, the crouch time and the drop.
const WHITE_BLOCK: [u8; 3] = [0x25, 0x26, 0x27];
pub const WHITE_BLOCK_TIME: u8 = 0xF0;
/// Sky flight starts (+0x27): from the note block, and arriving in the area above.
pub const SKY_LAUNCH: i8 = -0x80;
pub const SKY_ARRIVAL: i8 = -0x30;
const WHITE_BLOCK_DROP: i32 = 6;
// Swimming (0x08096EB4, 0x080974C8; table 0x08172C96 from index 0x40 on the floor of the
// water, 0x44 swimming): friction, acceleration, against the motion; the top speed is
// 0x08172C88[0] = 8 on the floor and 0x18 swimming.
const SWIM_FLOOR: [i32; 3] = [-0xD0, 0x100, 0x100];
const SWIM_AIR: [i32; 3] = [-0x20, 0x30, 0x80];
const SWIM_FLOOR_MAX: i32 = 8;
const SWIM_MAX: i32 = 0x18;
/// A stroke (A) sets the speed byte to -0x20 from the floor, or adds -0x20; it is held
/// within +/-0x20.
const STROKE: i32 = -0x20;
const SWIM_LIMIT: i32 = 0x20;
/// At the surface (0x0809588C) the rise is held to -0xC; Up + A jumps out at 0xCC (small)
/// or 0xCA.
const SURFACE_RISE: i32 = -0xC;
/// 0x080974C8: above -0x14 on the screen (+0x18) a rising swimmer is pushed by +0x10.
const SCREEN_TOP_REACH: i32 = 0x14;
const SCREEN_TOP_PUSH: i32 = 0x10;
const LEAP_SMALL: i32 = -0x34;
const LEAP: i32 = -0x36;
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
    /// Fire Mario (+0x37 = 2): throws fireballs with B.
    Fire,
    Raccoon,
}

impl Form {
    /// Box height: the ROM's head probes sit 16 px (small) / 6 px (others) below the top of
    /// a 32 px frame whose bottom is the feet, giving 16 and 26 px.
    pub const fn height(self) -> i32 {
        match self {
            Form::Small => 16,
            Form::Big | Form::Fire | Form::Raccoon => 26,
        }
    }

    /// Hits the player can take: a hit turns raccoon or fire into big and big into small
    /// (0x0804A5F4), and loses a life when small.
    pub const fn hit_points(self) -> u8 {
        match self {
            Form::Small => 1,
            Form::Big => 2,
            Form::Fire | Form::Raccoon => 3,
        }
    }

    /// The ROM's form number (+0x37), which also names the extracted sheets.
    pub const fn id(self) -> u8 {
        match self {
            Form::Small => 0,
            Form::Big => 1,
            Form::Fire => 2,
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
    /// Fire Mario throwing, step 0-2 (+0x67 >> 2), on the ground or in the air.
    Throw(u8),
    ThrowAir(u8),
    /// Transformation smoke, tile 0x0817388C[i] (0x83, 0x82, 0x81, 0x80); the player is
    /// not drawn.
    Puff(u8),
    /// Swimming: stroke step 0-5 (0x0813DE43), or paddling in place, step 0-3 (0x0813DE4F).
    Swim(u8),
    SwimIdle(u8),
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
            Pose::Throw(s) => a::PLAYER_THROW + s as usize,
            Pose::Swim(s) => a::PLAYER_SWIM + s as usize,
            Pose::SwimIdle(s) => a::PLAYER_SWIM_IDLE + s as usize,
            Pose::ThrowAir(s) => a::PLAYER_THROW_AIR + s as usize,
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
    /// Fire Flower flash (+0x3F = 0x1F, 0x0806B1E4): the player and the objects stop.
    pub flash: u8,
    /// Starman (+0x3D = 0xE0, 0x0806B058): -1 every other frame; invincible meanwhile.
    pub star: u8,
    /// Enemies knocked out by the star in a row (+0x107), for the points.
    pub star_chain: u16,
    /// Star palette step (+0x25, palette 0x08133210[+0x25 & 3]) and its timer (+0x24).
    pub star_palette: u8,
    star_timer: i8,
    /// Throw pose timer (+0x67 = 0xB when a fireball is thrown).
    pub throw: u8,
    /// Frames crouched on a white block (+0x49, 0x0809E164).
    white_block: u8,
    /// Sky flight (+0x27, negative while it runs): 0x80 from an orange note block, 0xD0
    /// arriving in the area above (0x080A8000).
    pub sky: i8,
    /// Behind the scenery (0x03003D06): 0xF0, -1 on odd frames, held at 1 while the player
    /// is inside scenery.
    pub behind: u8,
    /// Behind and inside scenery this frame (0x03003D05): enemies and their shots miss.
    pub hidden: bool,
    /// Course clear: has stood on the ground since touching the card (+0x43).
    goal_walking: bool,
    /// Skidding this frame: the skid pose, and the skid dust follows (0x08097800).
    pub skidding: bool,
    /// In water (+0x4C).
    pub in_water: bool,
    /// Strokes still showing (+0x4E) and the stroke's step (+0x54, stepped with the walk
    /// cycle).
    stroke: u8,
    stroke_step: u8,
    /// The swimming pose 0x08097724 picked this frame.
    swim_pose: Option<Pose>,
    /// Position (1/256 px) and ground flag at the start of this step and of the last one.
    /// The ROM moves the player by last frame's speed before its update; the port moves by
    /// the new speed after it, so its position runs a frame ahead. The water code
    /// (0x0809588C, the swimming handlers) decides on what the ROM sees, the state a step
    /// earlier, and a speed it changes redoes the last move (the ROM's move comes after it).
    now: Option<(i32, i32, bool)>,
    seen: (i32, i32, bool),
    /// The camera y as of last frame, set by the game: the draw (0x080A6E0C) leaves the
    /// player's screen y in +0x18 for the next update.
    pub camera_y: i32,
    /// Last step's handlers left the ground (a jump or stroke): the ROM's air flag changes
    /// at once, only the collision's lags.
    took_off: bool,
    /// Went into or out of the water this frame (0x0809F4C8): the splash follows.
    pub splash: Option<WaterChange>,
}

/// Into or out of the water.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaterChange {
    In,
    Out,
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
        self.frame_at(self.x, self.y)
    }

    /// The frame for a box at (x, y), 1/256 px.
    fn frame_at(&self, x: i32, y: i32) -> (i32, i32) {
        let (px, py) = (x.div_euclid(SUB), y.div_euclid(SUB));
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

    /// Fire Flower (0x0806B1E4): any other form -> fire (request +0x50 = 3) with the flash
    /// +0x3F = 0x1F; already fire, nothing but the points.
    pub fn fire_flower(&mut self) {
        if self.form != Form::Fire {
            self.set_form(Form::Fire);
            self.flash = FIRE_FLASH;
        }
    }

    /// Starman (0x0806B058): +0x3D = 0xE0, the chain restarts unless a star is running,
    /// palette timer +0x24 = 3, step +0x25 = 0.
    pub fn starman(&mut self) {
        if self.star == 0 {
            self.star_chain = 0;
        }
        self.star = STAR_TIME;
        self.star_palette = 0;
        self.star_timer = 3;
    }

    /// 0x08009180 (each frame the star runs): the palette steps every 4 frames while
    /// +0x3D >= 0x40, then every 6, then every 7 below 0x1E; plain again below 5.
    fn star_flash(&mut self) {
        if self.star == 1 {
            self.star_palette = 0;
            return;
        }
        self.star_timer -= 1;
        if self.star_timer < 0 {
            self.star_timer = match self.star {
                0x40.. => 3,
                0x1E..=0x3F => 5,
                _ => 6,
            };
            self.star_palette = self.star_palette.wrapping_add(1);
            if self.star < 5 {
                self.star_palette = 0;
            }
        }
    }

    /// 0x0808C068: Fire Mario throws on a new B press (the game checks the slots).
    pub fn wants_fireball(&self, pressed: Buttons) -> bool {
        self.form == Form::Fire && pressed.contains(Buttons::B) && !self.transforming()
    }

    /// Busy with a transformation: frozen, and hits are ignored (0x0804A5F4 returns early
    /// while +0x3B or +0x3E is set).
    pub fn transforming(&self) -> bool {
        self.morph != 0 || self.puff != 0 || self.flash != 0
    }

    /// Takes a hit. Returns false if the player had no hit points left.
    pub fn hurt(&mut self) -> bool {
        match self.form {
            Form::Raccoon | Form::Fire => self.set_form(Form::Big),
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
        if self.flash != 0 {
            self.flash -= 1;
            return None;
        }
        if self.puff != 0 {
            self.puff -= 1;
            return None;
        }
        if self.morph != 0 {
            self.morph -= 1;
            return None;
        }
        // 0x080A8000: sky flight: no speed across; the y speed byte is +0x27, which counts
        // up to 0 one a frame. In the ROM the player moves +0x27 + 1 a frame (checked in
        // mGBA: 126, 125, ... / 16 px after +0x27 = 0x80).
        let flying = self.sky < 0;
        if flying {
            self.vx = 0;
            self.sky += 1;
        }
        // Per-frame timers of the player loop (0x08094780: bytes +0x64..+0x70).
        self.p_timer = self.p_timer.saturating_sub(1);
        self.wag = self.wag.saturating_sub(1);
        self.whip = self.whip.saturating_sub(1);
        self.throw = self.throw.saturating_sub(1);
        // 0x080A6E0C: the star runs down on odd frames.
        if self.star != 0 {
            if frame & 1 != 0 {
                self.star -= 1;
            }
            if self.star == 0 {
                self.star_chain = 0;
            } else {
                self.star_flash();
            }
        }
        self.dither = (self.dither & 0xF0).wrapping_add(0x70);

        let last = self.now.replace((self.x, self.y, self.on_ground));
        self.seen = last.unwrap_or((self.x, self.y, self.on_ground));
        self.seen.2 &= !self.took_off;
        let grounded = self.on_ground;
        let mut pressed = pressed;
        let (vx, vy) = (self.vx, self.vy);
        self.splash = self.water(held, &mut pressed, map, frame);
        if (vx, vy) != (self.vx, self.vy) {
            self.x = self.seen.0 + self.vx;
            self.y = self.seen.1 + self.vy;
        }
        if self.in_water {
            // 0x0809588C steps the walk cycle (0x08097800) before the swimming handlers.
            self.animate_walk(held);
            self.swim_horizontal(held);
            self.swim_vertical(pressed, frame);
            self.swim_pose = None;
            if !self.seen.2 {
                self.stroke_pose(pressed, frame);
            }
        } else {
            self.horizontal(held);
            self.vertical(held, pressed);
        }
        self.took_off = grounded && !self.on_ground;
        if flying {
            self.vx = 0;
            self.vy = (self.sky as i32 + 1) * UNIT;
        }
        if self.is_raccoon() && !self.in_water {
            self.raccoon(pressed);
        }
        self.update_p_meter(frame);
        self.move_x(map);
        let head_hit = self.move_y(map);
        self.white_block(held, map);
        self.behind_scenery(map, frame);
        if self.on_ground && !self.in_water {
            self.animate_walk(held);
        }
        if self.on_ground && !self.in_water {
            self.skidding = self.skids(held);
        }
        head_hit
    }

    /// 0x0809588C: into the water when the head probe at frame (8, 0x14 small / 0x0A) is
    /// in it. With the head out of it: on the ground, out of the water; in the air, out once
    /// the foot probe at frame (8, 0x20) is in open air (tested while falling), otherwise
    /// at the surface: the rise is held to -0xC, A does nothing and Up + A jumps out.
    fn water(&mut self, held: Buttons, pressed: &mut Buttons, map: &Level, frame: u32) -> Option<WaterChange> {
        let (fx, fy) = self.frame_at(self.seen.0, self.seen.1);
        let ground = self.seen.2;
        let dy = if self.form == Form::Small { 0x14 } else { 0x0A };
        let (hx, hy) = ((fx + 8).div_euclid(TILE_PX), map.row_at(fy + dy));
        let head_solid = map.at(hx, hy) != TileKind::Empty;
        if !head_solid && map.is_water(hx, hy) {
            if self.in_water {
                return None;
            }
            self.in_water = true;
            return Some(self.enter_or_leave_water(WaterChange::In));
        }
        if !self.in_water {
            return None;
        }
        let vy = self.vy / UNIT;
        if ground {
            if head_solid {
                return None;
            }
            return Some(self.leave_water());
        }
        if head_solid || vy >= 0 {
            let (tx, ty) = ((fx + 8).div_euclid(TILE_PX), map.row_at(fy + 0x20));
            if map.at(tx, ty) == TileKind::Empty && !map.is_water(tx, ty) {
                return Some(self.leave_water());
            }
            if head_solid {
                return None;
            }
        }
        let mut rise = vy.max(SURFACE_RISE);
        if frame & 7 == 0 {
            rise += 1;
        }
        self.vy = rise * UNIT;
        pressed.remove(Buttons::A);
        if held.contains(Buttons::UP | Buttons::A) {
            self.vy = if self.form == Form::Small { LEAP_SMALL } else { LEAP } * UNIT;
            return Some(self.leave_water());
        }
        None
    }

    fn leave_water(&mut self) -> WaterChange {
        self.in_water = false;
        self.stroke = 0;
        self.enter_or_leave_water(WaterChange::Out)
    }

    /// 0x0809F4C8: falling in (or walking out) stops the fall, and the motion across if in
    /// the air; air bubbles come out (the game spawns them with the splash).
    fn enter_or_leave_water(&mut self, change: WaterChange) -> WaterChange {
        if self.vy >= 0 {
            self.vy = 0;
            if !self.seen.2 {
                self.vx = 0;
            }
        }
        change
    }

    /// 0x08096EB4 -> 0x08096D24 with the swimming table; friction applies in the water too.
    fn swim_horizontal(&mut self, held: Buttons) {
        let dir = match (held.contains(Buttons::LEFT), held.contains(Buttons::RIGHT)) {
            (true, false) => -1,
            (false, true) => 1,
            _ => 0,
        };
        let (table, target) = if self.seen.2 { (SWIM_FLOOR, SWIM_FLOOR_MAX) } else { (SWIM_AIR, SWIM_MAX) };
        let speed = self.vx / UNIT;
        if dir == 0 {
            if speed != 0 {
                self.accelerate(if speed < 0 { -table[0] } else { table[0] });
            }
            return;
        }
        self.facing_left = dir < 0;
        let delta = if speed != 0 && speed.signum() != dir {
            table[2]
        } else if speed.abs() == target {
            return;
        } else if speed.abs() < target {
            table[1]
        } else {
            table[0]
        };
        self.accelerate(delta * dir);
    }

    /// 0x080974C8: A strokes; in the water the speed byte rises by 1 a frame, or every
    /// other frame (bit 1 of the frame counter clear) while sinking.
    fn swim_vertical(&mut self, pressed: Buttons, frame: u32) {
        let mut vy = self.vy / UNIT;
        if pressed.contains(Buttons::A) {
            if self.seen.2 {
                vy = STROKE;
                self.on_ground = false;
                self.seen.2 = false;
            } else {
                vy += STROKE;
            }
        }
        if !self.seen.2 {
            if vy < 0 || frame & 2 == 0 {
                vy += 1;
            }
            // Rising more than 0x14 px above the top of the screen pushes back down.
            let screen_y = self.frame_at(self.seen.0, self.seen.1).1 - self.camera_y;
            if vy < 0 && screen_y < -SCREEN_TOP_REACH {
                vy += SCREEN_TOP_PUSH;
            }
            if !(-SWIM_LIMIT..SWIM_LIMIT).contains(&vy) {
                vy = if vy < 0 { -SWIM_LIMIT } else { SWIM_LIMIT };
            }
        }
        self.vy = vy * UNIT;
        self.flight = 0;
    }

    /// 0x08097724: a stroke starts on A (+0x4E counts the strokes to show, +0x54 restarts);
    /// each shows for six walk-cycle steps, the last step held while the count runs down.
    /// Without strokes and with no speed across, the paddling steps with the frame counter.
    fn stroke_pose(&mut self, pressed: Buttons, frame: u32) {
        if pressed.contains(Buttons::A) {
            if self.stroke == 0 {
                self.walk_timer = 0;
            }
            self.stroke_step = 0;
            self.stroke = self.stroke.saturating_add(1);
        }
        self.swim_pose = Some(if self.stroke == 0 {
            if self.vx / UNIT == 0 {
                self.walk_step = ((frame >> 4) & 3) as u8;
            }
            Pose::SwimIdle(self.walk_step)
        } else {
            if self.stroke_step > 5 {
                self.stroke -= 1;
            }
            Pose::Swim(self.stroke_step.min(5))
        });
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
        if self.throw != 0 && self.form == Form::Fire {
            // 0x08098648: 0x0813DE9E[(+0x67 >> 2) (+3 in the air)].
            let step = (self.throw >> 2).min(2);
            return if self.on_ground { Pose::Throw(step) } else { Pose::ThrowAir(step) };
        }
        // In water: the swimming pose the handler picked (before the collision, so it stays
        // on the frame he lands), or 0x08097800's walking pose when it did not run.
        if self.in_water {
            return self.swim_pose.unwrap_or(Pose::Walk(self.walk_step));
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
        // +0x43 is set the first time the player stands on the ground; from then on
        // 0x080A8000 walks right, also through the air (e.g. off the end of World 1-2's
        // ground), until the player is off the screen.
        self.goal_walking |= self.on_ground;
        if self.goal_walking {
            self.vx = GOAL_WALK * UNIT;
            self.facing_left = false;
            if self.on_ground {
                self.walk_step = ((frame & 6) >> 1) as u8;
            }
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
            self.stroke_step = self.stroke_step.wrapping_add(1);
        }
        if !held.contains(Buttons::LEFT) && !held.contains(Buttons::RIGHT) && speed == 0 {
            self.walk_step = STAND_STEP;
        }
    }

    fn move_x(&mut self, map: &Level) {
        self.x += self.vx;
        let (px, py) = self.px();
        let lift = if map.slopes.is_some() { SLOPE_WALL_LIFT } else { 0 };
        let rows = map.row_span(py, self.height() - lift);
        if self.vx > 0 {
            let tx = (px + HITBOX_W - 1).div_euclid(TILE_PX);
            if rows.iter().any(|&ty| map.at(tx, ty) == TileKind::Solid) {
                self.x = (tx * TILE_PX - HITBOX_W) * SUB;
                self.vx = 0;
            }
        } else if self.vx < 0 {
            let tx = px.div_euclid(TILE_PX);
            if rows.iter().any(|&ty| map.at(tx, ty) == TileKind::Solid) {
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
            let ty = map.row_at(py + h);
            let top = map.row_top(ty, py + h);
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
            let ty = map.row_at(py);
            let below = map.row_top(ty + 1, py);
            // Hidden blocks (World 1-5's 0x46): passable tiles with a bump behaviour stop
            // the head-centre probe from below only (checked in mGBA: the head turns there
            // and the tile becomes 0x71).
            let centre_tx = (px + HITBOX_W / 2).div_euclid(TILE_PX);
            if map.at(centre_tx, ty) == TileKind::Empty
                && map.metatile(centre_tx, ty).is_some_and(|m| {
                    m < 0x100 && map.rules.bump_behaviour[m as usize] != sma4_assets::BlockRules::NOT_BUMPABLE
                })
            {
                self.y = below * SUB;
                self.vy = 0;
                return Some((centre_tx, ty));
            }
            if cols.clone().any(|tx| map.at(tx, ty) == TileKind::Solid) {
                self.y = below * SUB;
                self.vy = 0;
                // The ROM's head-centre probe (probe 1) decides which block was hit.
                return (map.at(centre_tx, ty) == TileKind::Solid).then_some((centre_tx, ty));
            }
            None
        }
    }

    /// The feet probe (probe 0, frame + (0x20, 8)) on a tile that reacts to being stood on
    /// (note blocks: `BlockRules::land_behaviour`), while on the ground.
    pub fn feet_block(&self, map: &Level) -> Option<(i32, i32)> {
        if !self.on_ground {
            return None;
        }
        let (px, py) = self.px();
        let (tx, ty) = ((px + HITBOX_W / 2).div_euclid(TILE_PX), map.row_at(py + self.height()));
        let m = map.metatile(tx, ty)?;
        (m < 0x100 && map.rules.land_behaviour[m as usize] != sma4_assets::BlockRules::NOT_BUMPABLE).then_some((tx, ty))
    }

    /// 0x0809E164 (plains tilesets): standing on a white block (ids 0x25-0x27) with Down
    /// held for 0xF0 frames drops the player 6 px through it, behind the scenery.
    fn white_block(&mut self, held: Buttons, map: &Level) {
        let (px, py) = self.px();
        let feet = (py + self.height()).div_euclid(TILE_PX);
        let on_white = self.on_ground
            && tile_span(px, HITBOX_W).any(|tx| map.collision_id(tx, feet).is_some_and(|id| WHITE_BLOCK.contains(&id)));
        if !on_white || !held.contains(Buttons::DOWN) {
            self.white_block = 0;
            return;
        }
        self.white_block = self.white_block.wrapping_add(1);
        if self.white_block == WHITE_BLOCK_TIME {
            self.white_block = 0;
            self.behind = WHITE_BLOCK_TIME;
            self.vy = 0;
            self.y += WHITE_BLOCK_DROP * SUB;
            self.on_ground = false;
        }
    }

    /// 0x0809588C: the behind-the-scenery timer runs down on odd frames; while the tile at
    /// (frame x + 8, frame y + 0x14 small / 0x0A big) is scenery (not sky 0x80 or 0x41) the
    /// player stays behind, and is out of the enemies' reach.
    fn behind_scenery(&mut self, map: &Level, frame: u32) {
        self.hidden = false;
        if self.behind == 0 {
            return;
        }
        if frame & 1 != 0 {
            self.behind -= 1;
        }
        let (fx, fy) = self.frame();
        let dy = if self.form == Form::Small { 0x14 } else { 0x0A };
        let id = map.metatile((fx + 8).div_euclid(TILE_PX), (fy + dy).div_euclid(TILE_PX));
        let inside = id.is_some_and(|m| m != crate::level::SKY && m != 0x41);
        if inside {
            self.hidden = true;
            if self.behind == 0 {
                self.behind = 1;
            }
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

    /// Drops a player into `test_pool` at column `col` and lets it settle on the floor of
    /// the water.
    fn in_pool(col: i32, form: Form) -> Sim {
        let mut s = Sim { p: Player::spawn(col * TILE_PX, 0), map: crate::level::test_pool(), frame: 0, held: Buttons::empty() };
        s.p.set_form(form);
        s.run(Buttons::empty(), 300);
        s
    }

    #[test]
    fn falling_in_stops_the_fall_and_sinks_slowly_to_the_floor() {
        let mut s = Sim { p: Player::spawn(3 * TILE_PX, 0), map: crate::level::test_pool(), frame: 0, held: Buttons::empty() };
        let mut entered = None;
        for _ in 0..200 {
            s.run(Buttons::empty(), 1);
            if s.p.splash == Some(WaterChange::In) {
                entered = Some(s.p.vy);
            }
            if s.p.in_water {
                assert!(s.p.vy <= SWIM_LIMIT * UNIT, "sinks at most 0x20");
            }
        }
        assert_eq!(entered, Some(0), "0x0809F4C8 stops the fall on the way in");
        assert!(s.p.in_water && s.p.on_ground);
        assert_eq!(s.p.px().1 + s.p.height(), 7 * TILE_PX);
    }

    #[test]
    fn a_stroke_from_the_floor_sets_minus_0x20_and_a_second_adds_to_it_within_the_limit() {
        let mut s = in_pool(3, Form::Big);
        s.run(Buttons::A, 1);
        // -0x20, then +1 in the same frame (rising).
        assert_eq!(s.p.vy, (STROKE + 1) * UNIT);
        assert_eq!(s.p.pose(), Pose::Swim(0));
        s.run(Buttons::empty(), 1);
        s.run(Buttons::A, 1);
        // -0x1E - 0x20 + 1, held at -0x20.
        assert_eq!(s.p.vy, -SWIM_LIMIT * UNIT);
    }

    #[test]
    fn swimming_speed_tops_out_at_0x18_and_8_on_the_floor() {
        let mut s = in_pool(2, Form::Big);
        s.run(Buttons::RIGHT, 40);
        assert_eq!(s.p.vx, SWIM_FLOOR_MAX * UNIT);
        // 0x30 / 256 a frame from 8 to 0x18, stroking now and then to stay off the floor.
        for _ in 0..6 {
            s.run(Buttons::RIGHT | Buttons::A, 1);
            s.run(Buttons::RIGHT, 15);
        }
        assert!(!s.p.on_ground);
        assert_eq!(s.p.vx, SWIM_MAX * UNIT);
    }

    #[test]
    fn at_the_surface_a_does_nothing_and_up_a_jumps_out() {
        for (form, leap) in [(Form::Big, LEAP), (Form::Small, LEAP_SMALL)] {
            let mut s = in_pool(3, form);
            // Stroke up to the surface and stay there.
            for _ in 0..12 {
                s.run(Buttons::A, 1);
                s.run(Buttons::empty(), 7);
            }
            assert!(s.p.in_water, "{form:?}: A alone never leaves the water");
            assert!(s.p.px().1 + s.p.height() > 4 * TILE_PX, "{form:?}: held at the surface");
            let mut left = None;
            for _ in 0..40 {
                s.run(Buttons::UP | Buttons::A, 1);
                if s.p.splash == Some(WaterChange::Out) {
                    left = Some(s.p.vy);
                    break;
                }
                s.run(Buttons::empty(), 1);
            }
            // Out of the water the land gravity runs the same frame: +1 with A held.
            assert_eq!(left, Some((leap + GRAVITY_HOLD) * UNIT), "{form:?}");
            assert!(!s.p.in_water);
        }
    }

    #[test]
    fn walking_out_onto_the_ledge_leaves_the_water() {
        let mut s = in_pool(9, Form::Big);
        // The ledge is 3 tiles up: swim up and over it.
        let mut out = false;
        for _ in 0..30 {
            s.run(Buttons::RIGHT | Buttons::A | Buttons::UP, 1);
            s.run(Buttons::RIGHT, 3);
            out |= !s.p.in_water;
        }
        assert!(out);
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

    /// 10x6 map: a solid floor on row 5 and a row of white blocks (id 0x25) on row 3.
    fn white_blocks() -> Level {
        let (w, h) = (10, 6);
        let mut m = vec![crate::level::SKY; w * h];
        for x in 0..w {
            m[3 * w + x] = WHITE_BLOCK[0] as u16;
            m[5 * w + x] = 0x53;
        }
        Level::new(w, h, m, crate::level::TEST_SOLIDITY, sma4_assets::BlockRules::default())
    }

    #[test]
    fn holding_down_on_a_white_block_drops_behind_the_scenery() {
        let mut s = Sim::new(white_blocks(), 32);
        let top = 3 * TILE_PX - s.p.height();
        assert_eq!(s.p.px().1, top);
        // Letting go of Down starts the count again.
        s.run(Buttons::DOWN, WHITE_BLOCK_TIME as u32 - 1);
        s.run(Buttons::empty(), 1);
        s.run(Buttons::DOWN, WHITE_BLOCK_TIME as u32 - 1);
        assert_eq!((s.p.px().1, s.p.behind), (top, 0));
        s.run(Buttons::DOWN, 1);
        assert_eq!(s.p.behind, WHITE_BLOCK_TIME);
        assert!(!s.p.on_ground);
        // Through the block (inside it: out of the enemies' reach) and down to the floor.
        let mut hidden = 0;
        for _ in 0..62 {
            s.run(Buttons::empty(), 1);
            hidden += s.p.hidden as u32;
        }
        assert!(hidden > 0);
        assert_eq!(s.p.px().1, 5 * TILE_PX - s.p.height());
        assert!(!s.p.hidden);
        // The timer runs down on odd frames only.
        assert_eq!(s.p.behind, WHITE_BLOCK_TIME - 31);
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

    #[test]
    fn the_fire_flower_flashes_into_fire_mario_and_a_hit_leaves_him_big() {
        let mut s = Sim::new(floor(), 40);
        s.p.fire_flower();
        assert_eq!((s.p.form, s.p.flash), (Form::Fire, FIRE_FLASH));
        let x = s.p.x;
        s.run(Buttons::RIGHT, FIRE_FLASH as u32);
        assert_eq!(s.p.x, x, "frozen during the flash");
        assert!(!s.p.transforming());
        s.p.fire_flower();
        assert_eq!(s.p.flash, 0, "already fire: no flash");
        assert!(s.p.hurt());
        assert_eq!(s.p.form, Form::Big);
    }

    #[test]
    fn the_star_lasts_0xe0_odd_frames_and_its_palette_steps_slower_at_the_end() {
        let mut s = Sim::new(floor(), 40);
        s.p.starman();
        let mut frames = 0;
        let mut steps = Vec::new();
        let mut last = s.p.star_palette;
        while s.p.star != 0 {
            s.run(Buttons::empty(), 1);
            frames += 1;
            if s.p.star_palette != last {
                steps.push((s.p.star, frames));
                last = s.p.star_palette;
            }
        }
        assert!((2 * STAR_TIME as u32 - 1..=2 * STAR_TIME as u32 + 1).contains(&frames), "{frames}");
        // Every 4 frames early on, every 7 near the end.
        let gap = |w: &[(u8, u32)]| w[1].1 - w[0].1;
        assert_eq!(gap(&steps[1..3]), 4);
        let late: Vec<_> = steps.iter().filter(|s| (6..0x1E).contains(&s.0)).collect();
        assert!(late.windows(2).all(|w| w[1].1 - w[0].1 == 7), "{late:?}");
        assert_eq!(s.p.star_palette, 0);
    }

    #[test]
    fn fire_mario_wants_a_fireball_on_a_new_b_press_only() {
        let mut p = Player::spawn(0, 0);
        assert!(!p.wants_fireball(Buttons::B));
        p.set_form(Form::Fire);
        assert!(p.wants_fireball(Buttons::B));
        assert!(!p.wants_fireball(Buttons::A));
        p.throw = THROW_TIME;
        assert_eq!(p.pose(), Pose::ThrowAir(2));
    }
}

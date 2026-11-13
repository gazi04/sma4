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
/// Wading on the ground (0x08096884): the floor table up to 0x08172C88[0] << 1.
const WADE_MAX: i32 = 0x10;
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
/// Slope shapes that are the fill under a slope (0x0809A7E8: shapes 4 and 8): the feet
/// are looked up one cell higher.
const SLOPE_FILL: [u8; 2] = [4, 8];
/// Standing on nothing this deep into a cell, the feet are looked up one cell lower (so the
/// player follows a slope down); 0x0809A7E8.
const SLOPE_FOLLOW_DOWN: i32 = 10;
/// Sliding (+0x28, 0x0809A7E8): at most 0x40 either way.
const SLIDE_MAX: i32 = 0x40;
/// The frame's leftmost x in a level (px).
const LEFT_END: i32 = 4;
/// The x move's limit (0x080A35A0: player +0x8A >> 2, +0x8A = 0x100).
const MOVE_MAX: i32 = 0x40;
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
/// +0x42 on a turn with a shell (0x08097ACC).
const HOLD_TURN_FRAMES: u8 = 7;
/// +0x41 after a kick (0x080436A0).
pub const KICK_POSE_FRAMES: u8 = 0x0C;
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
    /// Sliding down a slope (0x08097A94).
    Slide,
    /// Crouching (+0x48).
    Crouch,
    /// Crouched in the air, step 0-2 (the raccoon's tail wag +0x65 >> 2).
    CrouchAir(u8),
    /// Fire Mario throwing, step 0-2 (+0x67 >> 2), on the ground or in the air.
    Throw(u8),
    ThrowAir(u8),
    /// Transformation smoke, tile 0x0817388C[i] (0x83, 0x82, 0x81, 0x80); the player is
    /// not drawn.
    Puff(u8),
    /// Swimming: stroke step 0-5 (0x0813DE43), or paddling in place, step 0-3 (0x0813DE4F).
    Swim(u8),
    SwimIdle(u8),
    /// Hanging on the airship's anchor (0x081737CC[form], 0x080A855C).
    Hang,
    /// Carrying a shell (0x0809EEB4): walk step 0-3 (0x0813DEC0[0x0813DD6F[form] + step]);
    /// in the air step 0 (1 small), the raccoon wagging its tail 0x0813DF00[+0x65 >> 2].
    Hold(u8),
    HoldWag(u8),
    /// Turning round with a shell (+0x42, 0x08097ACC: pose 0x32 small, 0x05 the others).
    HoldTurn,
    /// Kicking a shell (+0x41, 0x08097ACC: pose 0x42 small, 0x2D the others).
    Kick,
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
            Pose::Slide => a::PLAYER_SLIDE,
            Pose::Crouch => a::PLAYER_CROUCH,
            Pose::CrouchAir(s) => a::PLAYER_CROUCH_AIR + s as usize,
            Pose::Throw(s) => a::PLAYER_THROW + s as usize,
            Pose::Swim(s) => a::PLAYER_SWIM + s as usize,
            Pose::SwimIdle(s) => a::PLAYER_SWIM_IDLE + s as usize,
            Pose::Hang => a::PLAYER_HANG,
            Pose::Hold(s) => a::PLAYER_HOLD + s as usize,
            Pose::HoldWag(s) => a::PLAYER_HOLD_WAG + s as usize,
            Pose::HoldTurn => a::PLAYER_HOLD_TURN,
            Pose::Kick => a::PLAYER_KICK,
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
    /// A pose set by a scripted mode (the airship's intro, +0x45) instead of the usual one.
    pub forced_pose: Option<Pose>,
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
    /// Push timer +0x70 (8 after Larry's shell pushes the player, 0x080735DC): while it
    /// runs, the wall push-out 0x0809E664 snaps the frame to the 16 px grid in one go
    /// instead of 1 px a frame, and clears it.
    pub push_timer: u8,
    /// Tanooki statue timer +0x52 (Down + B as a Tanooki, 0x08097FE0). The port has no
    /// Tanooki suit yet, so nothing sets it; the wand reacts to it (`wand`).
    pub statue: u8,
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
    /// Standing on a slope that pushes against the motion (0x03003CE4, set by the slope
    /// pass 0x0809A7E8 on landing, read by the next frame's speed update).
    uphill: bool,
    /// The push of the slope stood on (0x0816A179[shape], player +0x94), from the last
    /// landing.
    push: i32,
    /// Sliding down a slope (+0x28): the speed byte, set as the speed every frame.
    pub slide: i32,
    /// The slope pass stopped a rise at a ceiling: the ROM's collision comes after the speed
    /// update, so the stop is applied after the next one (`vertical`).
    bonk: bool,
    /// How far the last slope pass moved a grounded player to the floor (1/256 px). The
    /// ROM's jump (or stroke) comes before its collision, which then leaves a rising player
    /// where the move put him; the port's collision ran a step earlier, so a take-off takes
    /// it back.
    floor_snap: i32,
    /// A landing the last slope pass held back only because the player was rising: the
    /// floor y (1/256 px) and its push. The ROM's collision reads the speed after the
    /// next update, so the landing happens if that one no longer rises (`land_late`).
    late_landing: Option<(i32, i32)>,
    /// Stopped at the level's left end (`move_x`).
    at_left_end: bool,
    /// The position after the last move, before its collision: what the ROM's next update
    /// probes (it moves, updates, then collides).
    moved: (i32, i32),
    /// The frame counter of this step; the slope pass probes the walls on alternate sides
    /// with it while standing still.
    clock: u32,
    /// The slide pose this frame (0x08097A94).
    slide_pose: bool,
    /// Crouching (+0x48, `crouch`): the collision is the small form's.
    pub crouch: bool,
    /// Carrying a shell (0x03007820 + 0, set each frame by the shell, 0x08043434).
    pub carrying: bool,
    /// B is held and nothing is carried yet: touching a resting shell from the side picks
    /// it up (0x08048E8C; set by the game).
    pub grab: bool,
    /// +0x42: frames of the turn with a shell (7 when the facing changes, 0x08097ACC), and
    /// whether this frame showed it.
    pub hold_turn: u8,
    turning: bool,
    /// +0x41: frames of the kick pose (0x0C from 0x080436A0), and whether this frame shows it.
    pub kick: u8,
    kicking: bool,
    /// Standing on an object (0x030038C5), with its speed byte across; set by the game.
    pub riding: Option<i32>,
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
        if self.crouch { Form::Small.height() } else { self.form.height() }
    }

    /// The small form's probes and box: small, or crouching (+0x48).
    pub fn small_box(&self) -> bool {
        self.form == Form::Small || self.crouch
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

    /// Puts the frame's top-left at (fx, fy), 1/256 px (the ROM's +0 / +4).
    pub fn set_frame_sub(&mut self, fx: i32, fy: i32) {
        self.x = fx + (FRAME_W / 2 - HITBOX_W / 2) * SUB;
        self.y = fy + (FRAME_H - self.height()) * SUB;
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
        self.set_crouch(false);
        self.y += (self.form.height() - form.height()) * SUB;
        self.form = form;
        if form != Form::Raccoon {
            self.flight = 0;
            self.whip = 0;
        }
    }

    /// Entering a level from the map (0x08094E4C in sub-state 0x19): standing if the probe
    /// at frame (8, 0x21) is at least top-solid, and the header's start kind becomes the
    /// slide byte (+0x28), so World 1-5 (kind 1) starts with Mario already sliding, in the
    /// slide pose.
    pub fn start(&mut self, kind: u8, map: &Level) {
        let (fx, fy) = self.frame();
        self.on_ground = map.at((fx + 8).div_euclid(TILE_PX), (fy + 0x21).div_euclid(TILE_PX)) != TileKind::Empty;
        self.slide = kind as i32;
        self.slide_pose = kind == 1;
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
        self.push_timer = self.push_timer.saturating_sub(1);
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
        self.clock = frame;

        let last = self.now.replace((self.x, self.y, self.on_ground));
        self.seen = last.unwrap_or((self.x, self.y, self.on_ground));
        self.seen.2 &= !self.took_off;
        let grounded = self.on_ground;
        self.crouch_check(held, map);
        let mut pressed = pressed;
        let (vx, vy) = (self.vx, self.vy);
        self.splash = self.water(held, &mut pressed, map, frame);
        if (vx, vy) != (self.vx, self.vy) {
            self.x = self.seen.0 + self.vx;
            self.y = self.seen.1 + self.vy;
        }
        if self.in_water {
            // 0x0809A7E8: no sliding in the water, but a landing still sets the uphill flag.
            self.slide = 0;
            // 0x0809588C steps the walk cycle (0x08097800) before the swimming handlers.
            self.animate_walk(held);
            self.swim_horizontal(held);
            self.swim_vertical(pressed, frame);
            self.land_late();
            if self.on_ground {
                self.set_uphill();
            }
            self.swim_pose = None;
            if !self.seen.2 {
                self.stroke_pose(pressed, frame);
            }
        } else {
            self.horizontal(held, map);
            self.vertical(held, pressed);
            self.land_late();
            self.slide(held);
        }
        self.took_off = grounded && !self.on_ground;
        if self.took_off {
            self.y -= self.floor_snap;
        }
        if flying {
            self.vx = 0;
            self.vy = (self.sky as i32 + 1) * UNIT;
        }
        if self.is_raccoon() && !self.in_water {
            self.raccoon(pressed);
        }
        self.update_p_meter(frame);
        // The ROM's next water check comes before its move: falling in stops the fall (and
        // in the air the motion across), at the surface the rise is held (`surface_hold`),
        // and the move goes by that; the speeds themselves change next step (`water`).
        let (x, y, ground) = self.now.unwrap_or((self.x, self.y, self.on_ground));
        let speeds = (self.vx, self.vy);
        if !self.in_water && self.head_in_water(map, x, y) {
            if self.vy >= 0 {
                self.vy = 0;
                if !ground {
                    self.vx = 0;
                }
            }
        } else if let Some(rise) = self.surface_hold(map, x, y, ground, frame.wrapping_add(1)) {
            self.vy = rise * UNIT;
        }
        let moved_by = (self.vx, self.vy);
        self.move_x(map);
        let head_hit = self.move_y(map);
        if self.vx == moved_by.0 {
            self.vx = speeds.0;
        }
        if self.vy == moved_by.1 {
            self.vy = speeds.1;
        }
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
        let dy = if self.small_box() { 0x14 } else { 0x0A };
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
            self.vy = if self.small_box() { LEAP_SMALL } else { LEAP } * UNIT;
            return Some(self.leave_water());
        }
        None
    }

    /// `water`'s head probe for a box at (x, y): in a water cell that does not collide.
    fn head_in_water(&self, map: &Level, x: i32, y: i32) -> bool {
        let (fx, fy) = self.frame_at(x, y);
        let dy = if self.small_box() { 0x14 } else { 0x0A };
        let (hx, hy) = ((fx + 8).div_euclid(TILE_PX), map.row_at(fy + dy));
        map.at(hx, hy) == TileKind::Empty && map.is_water(hx, hy)
    }

    /// The surface part of `water` for a frame at box (x, y) with ground flag `ground`: in
    /// the water, airborne, the head probe out of it and not in ground, and (falling) the
    /// foot probe still in water or ground: the speed byte held to -0xC, +1 when
    /// `frame & 7 == 0`.
    fn surface_hold(&self, map: &Level, x: i32, y: i32, ground: bool, frame: u32) -> Option<i32> {
        if !self.in_water || ground {
            return None;
        }
        let (fx, fy) = self.frame_at(x, y);
        let dy = if self.small_box() { 0x14 } else { 0x0A };
        let (hx, hy) = ((fx + 8).div_euclid(TILE_PX), map.row_at(fy + dy));
        if map.at(hx, hy) != TileKind::Empty || map.is_water(hx, hy) {
            return None;
        }
        let vy = self.vy / UNIT;
        if vy >= 0 {
            let (tx, ty) = ((fx + 8).div_euclid(TILE_PX), map.row_at(fy + 0x20));
            if map.at(tx, ty) == TileKind::Empty && !map.is_water(tx, ty) {
                return None;
            }
        }
        Some(vy.max(SURFACE_RISE) + (frame & 7 == 0) as i32)
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
    fn horizontal(&mut self, held: Buttons, map: &Level) {
        // The ROM's speed update reads the air flag its last collision left: the one the
        // port's collision left a step earlier (its position runs a frame ahead).
        let ground = self.seen.2;
        let dir = match (held.contains(Buttons::LEFT), held.contains(Buttons::RIGHT)) {
            (true, false) => -1,
            (false, true) => 1,
            _ => 0,
        };
        let speed = self.vx / UNIT;
        let run = held.contains(Buttons::B);
        let wading = self.wading(map);
        let target = match (run, self.p_meter == P_FULL) {
            _ if wading => if ground { WADE_MAX } else { SWIM_MAX },
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
        if run && ground && !wading && !self.uphill && self.stuck == 0 && self.slide == 0 {
            self.running = speed.abs() >= RUN_MAX;
            if speed.abs() >= WALK_MAX && dir != 0 && dir == speed.signum() && self.p_timer == 0 && self.p_meter > 0x3E {
                self.fly_ready = true;
            }
        }
        let (friction, accel, skid) = match () {
            _ if wading => {
                let t = if ground { SWIM_FLOOR } else { SWIM_AIR };
                (t[0], t[1], t[2])
            }
            _ if self.form == Form::Small => (FRICTION_SMALL, ACCEL, SKID),
            _ => (FRICTION, ACCEL, SKID),
        };
        if dir == 0 {
            if ground && speed != 0 {
                self.accelerate(if speed < 0 { -friction } else { friction });
            }
            return;
        }
        if self.whip == 0 {
            self.facing_left = dir < 0;
        }
        let delta = if speed != 0 && speed.signum() != dir {
            skid
        } else if speed.abs() == target {
            return;
        } else if speed.abs() < target {
            accel
        } else if !ground {
            return;
        } else {
            friction
        };
        self.accelerate(delta * dir);
    }

    /// 0x08096884: wading, out of the water (+0x4C) with the probe at frame (8, 0x1D) in it
    /// and the one at (8, 0x14 small / 8) not (`Level::player_water`).
    fn wading(&self, map: &Level) -> bool {
        let (fx, fy) = self.frame_at(self.moved.0, self.moved.1);
        let water = |dy: i32| map.player_water((fx + 8).div_euclid(TILE_PX), (fy + dy).div_euclid(TILE_PX));
        water(0x1D) && !water(if self.small_box() { 0x14 } else { 8 })
    }

    /// 0x08096F9C: jump, gravity.
    fn vertical(&mut self, held: Buttons, pressed: Buttons) {
        let mut vy = self.vy / UNIT;
        // As in `horizontal`: the air flag of the ROM's last collision. Its collision comes
        // after this update, so a landing (the port's last collision) still stops the fall.
        let mut ground = self.seen.2;
        let landed = self.on_ground;
        if ground && pressed.contains(Buttons::A) {
            let speed = (self.vx / UNIT).unsigned_abs() as usize;
            vy = JUMP_BASE - JUMP_BONUS[(speed >> 4).min(7)];
            self.on_ground = false;
            ground = false;
            self.flutter = 0;
            // 0x0809D72C: a full P-meter and a running start lets the raccoon take off.
            if self.p_meter == P_FULL && self.fly_ready {
                self.fly_ready = false;
                self.flight = FLIGHT_TIME;
            }
        }
        if ground || (landed && self.on_ground) {
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
        // A ceiling met in the last collision (`slope_pass`) stops what is left of the rise.
        if std::mem::take(&mut self.bonk) && vy < 0 {
            vy = 0;
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

    /// Whether the ROM's projectiles (0x0808EAA4) see the tail on a hitting frame
    /// (0x08169D88[+0x6A]: 8, 9, 12, 13). The ROM sets +0x6A = 0x12 before its timer loop
    /// takes 1 off in the same frame (mGBA: 17 after the press frame), so its +0x6A is
    /// `whip` - 1 when they run.
    pub fn tail_hitting_for_projectiles(&self) -> bool {
        self.is_raccoon() && matches!(self.whip.wrapping_sub(1), 8 | 9 | 12 | 13)
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

    /// 0x08097ACC's timers, after the player's update: the kick pose (+0x41) and, on the
    /// ground with a shell, the turn (+0x42 = 7 when the facing changed this frame, then
    /// down by 1 a frame, the turn pose while it ran).
    pub fn step_hold(&mut self, was_left: bool) {
        self.kicking = self.kick != 0;
        self.kick = self.kick.saturating_sub(1);
        if self.hold_turn == 0 && self.carrying && self.on_ground && self.facing_left != was_left {
            self.hold_turn = HOLD_TURN_FRAMES;
        }
        self.turning = self.hold_turn != 0 && self.carrying;
        self.hold_turn = self.hold_turn.saturating_sub(1);
        if !self.carrying {
            self.hold_turn = 0;
        }
    }

    /// Sprite pose, following the ROM's pose routines (0x08097800, 0x08097ACC,
    /// 0x080981C4, 0x080984E8, 0x080983F8).
    pub fn pose(&self) -> Pose {
        if let Some(p) = self.forced_pose {
            return p;
        }
        if self.puff == 0 && self.morph == 0 && !self.in_water {
            // 0x08097ACC: the kick pose last, over everything else; before it the shell's
            // poses (0x0809EEB4) replace the walking, running and jumping ones.
            if self.kicking {
                return Pose::Kick;
            }
            if self.carrying {
                if self.turning {
                    return Pose::HoldTurn;
                }
                if self.on_ground {
                    return Pose::Hold(self.walk_step);
                }
                return match self.form {
                    Form::Raccoon => Pose::HoldWag((self.wag >> 2).min(2)),
                    Form::Small => Pose::Hold(1),
                    _ => Pose::Hold(0),
                };
            }
        }
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
        // 0x08097A94 (from 0x08097800, before the handlers): the slide pose stays on the
        // frame he slides off an edge; a jump's handler replaces it.
        if self.slide_pose && (self.on_ground || self.vy >= 0) {
            return Pose::Slide;
        }
        // 0x080981C4: a crouching raccoon in the air shows 0x0813DE9B[+0x65 >> 2].
        if self.crouch {
            return if self.on_ground { Pose::Crouch } else { Pose::CrouchAir((self.wag >> 2).min(2)) };
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
        if falling && matches!(self.form, Form::Big | Form::Fire) { Pose::Fall } else { Pose::Jump }
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

    /// Sliding down a slope (+0x28). 0x08097800 ends a slide in the air or with Left, Right
    /// or Up held (and shows the slide pose, 0x08097A94: 0x33 small, 0x31 others); then
    /// 0x0809A7E8, on the ground: Down alone on a slope that pushes (0x0816A179: 45 degrees
    /// +/-3, gentle +/-2) starts one; each frame the push is added (not while going up a
    /// slope against the slide), the slide is held within +/-0x40 and loses 1, and it is
    /// the speed. On flat ground it runs down by 1 a frame.
    fn slide(&mut self, held: Buttons) {
        self.slide_pose = self.slide != 0;
        // 0x08097800 runs before the handlers: the air flag of the ROM's last collision, as
        // in `horizontal`.
        if self.slide != 0
            && (!self.seen.2 || held.contains(Buttons::LEFT) || held.contains(Buttons::RIGHT) || held.contains(Buttons::UP))
        {
            self.slide = 0;
        }
        if !self.on_ground {
            return;
        }
        self.set_uphill();
        let (p, mut s) = (self.push, self.slide);
        let against = s != 0 && (if s < 0 { -p } else { p }) < 0;
        if !against {
            if s == 0 {
                // (0x03003AEA & 0xB0) == 0x80: Down, without Left or Right.
                let down_only = held.contains(Buttons::DOWN) && !held.contains(Buttons::LEFT) && !held.contains(Buttons::RIGHT);
                if p == 0 || !down_only {
                    return;
                }
                self.slide_pose = true;
            }
            s = (s + p).clamp(-SLIDE_MAX, SLIDE_MAX);
        }
        s += if s < 0 { 1 } else { -1 };
        self.slide = s;
        self.vx = s * UNIT;
    }

    /// 0x03003CE4, for the next frame's speed: the push against the motion (a speed of 0
    /// counts as rightward). Set only on landing: a jump keeps it.
    fn set_uphill(&mut self) {
        self.uphill = self.push != 0 && (self.push < 0) != (self.vx < 0);
    }

    /// 0x0809588C, first thing: big forms crouch (+0x48) on the ground with Down alone on
    /// the pad, not while sliding, nor where the slope pushes a moving player (+0x46, so on
    /// a slope Down slides); a crouch lasts through a jump and ends in the water. Not with
    /// Micro-Goombas on.
    fn crouch_check(&mut self, held: Buttons, map: &Level) {
        let pad = held & (Buttons::DOWN | Buttons::UP | Buttons::LEFT | Buttons::RIGHT);
        let crouch = if self.form == Form::Small || self.slide != 0 || self.stuck != 0 {
            false
        } else if self.seen.2 {
            !(map.has_slopes() && self.push != 0 && self.vx != 0) && (pad == Buttons::DOWN || self.held_down(map))
        } else {
            self.crouch && !self.in_water
        };
        self.set_crouch(crouch);
    }

    /// 0x0809F0F4: crouched on an object, ground at frame (8, 0x0A) keeps the player down,
    /// as does ground at (0x0E, 0x0E) on one moving right, (1, 0x0E) moving left.
    fn held_down(&self, map: &Level) -> bool {
        let Some(speed) = self.riding.filter(|_| self.crouch) else { return false };
        let (fx, fy) = self.frame();
        let solid = |dx: i32, dy: i32| map.at((fx + dx).div_euclid(TILE_PX), (fy + dy).div_euclid(TILE_PX)) == TileKind::Solid;
        solid(8, 0x0A) || (speed != 0 && solid(if speed > 0 { 0x0E } else { 1 }, 0x0E))
    }

    /// Crouches or stands, keeping the feet where they are.
    fn set_crouch(&mut self, crouch: bool) {
        if crouch != self.crouch {
            let h = self.height();
            self.crouch = crouch;
            self.y += (h - self.height()) * SUB;
        }
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

    /// The x move (0x0809588C -> 0x080A35A0): on a slope, moving, the slope's push (+0x46,
    /// kept by 0x0809A7E8 while the speed is not 0) is added for the move: downhill faster,
    /// uphill slower. The sum is held within +/-0x40 (+0x8A >> 2) and the speed keeps what
    /// is left of it once the push is taken off again.
    fn move_x(&mut self, map: &Level) {
        // In the water 0x0809A7E8 returns before keeping it, and a jump leaves no landing.
        // Stopped at the level's left end, the speed stays 0 while it points into it.
        if self.at_left_end && self.vx < 0 {
            self.vx = 0;
        }
        let from = self.frame().0;
        let push = if self.vx != 0 && !self.in_water && self.on_ground { self.push } else { 0 };
        let v = self.vx / UNIT + push;
        if v.abs() >= MOVE_MAX {
            self.vx = (v.signum() * MOVE_MAX - push) * UNIT;
        }
        self.x += self.vx + push * UNIT;
        // The level's left end (checked in mGBA, 1-1 and 1-2, whatever the camera): moving
        // left from a frame at x 4 or less stops the speed, a frame left of 4 is put back at
        // 4 (its fraction kept), and Left no longer moves the player until he turns away.
        if self.vx < 0 && from <= LEFT_END {
            let fx = self.frame().0;
            if fx < LEFT_END {
                self.x += (LEFT_END - fx) * SUB;
            }
            self.vx = 0;
            self.at_left_end = true;
        } else if self.vx > 0 {
            self.at_left_end = false;
        }
        if map.has_slopes() {
            // The slope pass (`slope_pass`) handles the walls; the right end stops the
            // player as before.
            let max = (map.pixel_width() - HITBOX_W) * SUB;
            if self.x > max {
                self.x = max;
                self.vx = 0;
            }
            return;
        }
        let (px, py) = self.px();
        let rows = map.row_span(py, self.height());
        if self.vx > 0 {
            let tx = (px + HITBOX_W - 1).div_euclid(TILE_PX);
            if rows.iter().any(|&ty| map.at(tx, ty) == TileKind::Solid) {
                let moved = self.frame_x_sub();
                self.x = (tx * TILE_PX - HITBOX_W) * SUB;
                self.vx = 0;
                self.pushed_snap(-1, moved);
            }
        } else if self.vx < 0 {
            let tx = px.div_euclid(TILE_PX);
            if rows.iter().any(|&ty| map.at(tx, ty) == TileKind::Solid) {
                let moved = self.frame_x_sub();
                self.x = (tx + 1) * TILE_PX * SUB;
                self.vx = 0;
                self.pushed_snap(1, moved);
            }
        }
    }

    /// 0x0809E664 with the push timer (+0x70) running: the frame pushed out of a wall goes
    /// to the 16 px grid (x & ~0xFFF, + 0x1000 pushing right) rather than to where the side
    /// probe lines up with the wall; the timer is cleared. `moved` is the frame x (1/256
    /// px) after the move, before the port's own push-out: the ROM snaps that.
    fn pushed_snap(&mut self, dir: i32, moved: i32) {
        if self.push_timer == 0 {
            return;
        }
        self.push_timer = 0;
        let snapped = (moved & !0xFFF) + if dir > 0 { 0x1000 } else { 0 };
        self.x += snapped - self.frame_x_sub();
    }

    /// The frame's x in 1/256 px (the ROM's +0).
    fn frame_x_sub(&self) -> i32 {
        self.x + (HITBOX_W / 2 - FRAME_W / 2) * SUB
    }

    fn move_y(&mut self, map: &Level) -> Option<(i32, i32)> {
        let h = self.height();
        let old_bottom = self.y.div_euclid(SUB) + h;
        self.y += self.vy;
        let (px, py) = self.px();
        let cols = tile_span(px, HITBOX_W);
        let grounded = self.on_ground;
        self.on_ground = false;
        self.push = 0;
        self.moved = (self.x, self.y);
        if map.has_slopes() {
            return self.slope_pass(map, grounded);
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
        let dy = if self.small_box() { 0x14 } else { 0x0A };
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

    /// The ROM's collision for tilesets with slopes, 0x0809A7E8, on the 16x32 frame (as
    /// 1/256 px from here on). Probes (0x08172DB7, (y, x) from the frame's top-left; x 3
    /// for the left side, 0xD the right, by the motion, by the frame counter when still):
    /// 0 feet (0x20, 8), 1 head (5, 8; small 0x12), 2 / 3 walls (0x18, side) and (0xC, side;
    /// small 0x17); a probe reads the cell's slope shape (0x080A3760) or, for the walls, its
    /// solidity.
    /// * Feet on the fill under a slope (shapes 4, 8): the frame goes up so the feet are on
    ///   the cell above's last line, probed again. On the ground with nothing under the feet
    ///   and the feet 10 or more into their cell: down so the feet are on the next cell's top
    ///   line, probed again (the player follows a slope down). Otherwise, with the head in
    ///   ground (shapes 4, 7): down to the next cell line under the head.
    /// * Walls: either wall probe solid pushes the frame 1 px a frame until the side probe
    ///   lines up with the cell edge (toward the left in the cell's left half), and moving
    ///   into the push stops the speed and any slide.
    /// * Floor (the feet's shape has one, 0x08172DE3): not while rising unless sliding; in
    ///   the air not rising faster than -0x1C and only once the feet reach the height map's
    ///   floor; then the feet's line in the cell becomes the floor and the player lands
    ///   (0x080A3A5C, which drops the fraction). The push of the shape is kept (`slide`, `move_x`).
    /// * No floor: with the head on a shape (a ceiling slope, 5 / 6 / 0x13 / 0x14, or
    ///   solid), the head is pushed down to the ceiling line of the height map (high nibble)
    ///   when it is above it, stopping a rise (sound 0x60). With nothing at the head, on the
    ///   ground: a last probe at (0x20, 0xB) in the cell's left half, (0x20, 4) in its right,
    ///   keeps the player standing if it is solid ground (shape 3); otherwise he falls.
    fn slope_pass(&mut self, map: &Level, grounded: bool) -> Option<(i32, i32)> {
        let h = self.height();
        let small = self.small_box();
        self.floor_snap = 0;
        self.late_landing = None;
        let mut fx = self.x + (HITBOX_W / 2 - FRAME_W / 2) * SUB;
        let mut fy = self.y + (h - FRAME_H) * SUB;
        let px = |v: i32| v.div_euclid(SUB);
        let cell = |x: i32, y: i32| (x.div_euclid(TILE_PX), y.div_euclid(TILE_PX));
        let shape_at = |fx: i32, fy: i32, dy: i32, dx: i32| {
            let (tx, ty) = cell(px(fx) + dx, px(fy) + dy);
            map.slope(tx, ty)
        };
        let right = self.vx > 0 || (self.vx == 0 && self.clock & 1 != 0);
        let side = if right { 0xD } else { 3 };
        let upper = if small { 0x17 } else { 0xC };
        let head_dy = if small { 0x12 } else { 5 };
        let wall = |fy: i32, dy: i32| {
            let (tx, ty) = cell(px(fx) + side, px(fy) + dy);
            map.at(tx, ty) == TileKind::Solid
        };
        // All four probes come first; the wall test reads them later.
        let walls = wall(fy, upper) || wall(fy, 0x18);
        let mut feet = shape_at(fx, fy, 0x20, 8);
        let mut head = shape_at(fx, fy, head_dy, 8);
        // Hidden blocks (World 1-5's 0x46): passable tiles with a bump behaviour stop the
        // head-centre probe from below (checked in mGBA: the head turns there and the tile
        // becomes 0x71).
        if self.vy < 0 {
            let (hx, hy) = self.px();
            let (tx, ty) = ((hx + HITBOX_W / 2).div_euclid(TILE_PX), map.row_at(hy));
            if map.at(tx, ty) == TileKind::Empty
                && map.metatile(tx, ty).is_some_and(|m| {
                    m < 0x100 && map.rules.bump_behaviour[m as usize] != sma4_assets::BlockRules::NOT_BUMPABLE
                })
            {
                self.y = map.row_top(ty + 1, hy) * SUB;
                self.vy = 0;
                return Some((tx, ty));
            }
        }
        let line = |fy: i32, to: i32| (fy & !(0xF * SUB)) + to * SUB;
        // Where a rising player would be left (see `floor_snap`).
        let mut air_y = self.y;
        if SLOPE_FILL.contains(&feet) {
            fy = line(fy, -1);
            feet = shape_at(fx, fy, 0x20, 8);
            air_y = fy - (h - FRAME_H) * SUB;
        } else if grounded && feet == 0 && px(fy) & 0xF >= SLOPE_FOLLOW_DOWN {
            fy = line(fy, TILE_PX);
            feet = shape_at(fx, fy, 0x20, 8);
        } else if head == 4 || head == 7 {
            fy += ((-(head_dy + px(fy))) & 0xF) * SUB;
            // 0x0809B508 with 0x08172E02: a small player's feet, a big one's head at 0x12.
            if small {
                feet = shape_at(fx, fy, 0x20, 8);
            } else {
                head = shape_at(fx, fy, 0x12, 8);
            }
        }
        if walls {
            let left_half = px(fx) & 0xF < 8;
            let (dir, edge) = if left_half { (-1, 0xD) } else { (1, 4) };
            if (px(fx) + edge) & 0xF != 0 {
                fx += dir * SUB;
                // 0x0809E664 with +0x70: to the 16 px grid at once.
                if self.push_timer != 0 {
                    self.push_timer = 0;
                    fx = (fx & !0xFFF) + if dir > 0 { 0x1000 } else { 0 };
                }
                if (dir < 0) == (self.vx >= 0) {
                    self.vx = 0;
                    self.slide = 0;
                }
            }
        }
        self.x = fx - (HITBOX_W / 2 - FRAME_W / 2) * SUB;
        self.y = fy - (h - FRAME_H) * SUB;
        let col = px(fx) + 8;
        let Some(floor) = map.slope_floor(feet, col) else {
            if head != 0 {
                return self.ceiling(map, head, head_dy, col, fy);
            }
            if grounded {
                let dx = if px(fx) & 0xF < 8 { 0xB } else { 4 };
                if shape_at(fx, fy, 0x20, dx) == 3 {
                    self.on_ground = true;
                }
            }
            return None;
        };
        // 0x080A3A5C drops the fraction.
        let floor_y = (line(fy, floor) & !(SUB - 1)) - (h - FRAME_H) * SUB;
        if !grounded && px(fy) & 0xF < floor {
            return None;
        }
        if (self.slide == 0 && self.vy < 0) || (!grounded && self.vy < -0x1C * UNIT) {
            if !grounded {
                self.late_landing = Some((floor_y, map.slope_push(feet)));
            }
            return None;
        }
        self.y = floor_y;
        self.vy = 0;
        self.on_ground = true;
        self.flutter = 0;
        self.stomp_chain = 0;
        self.push = map.slope_push(feet);
        if grounded {
            self.floor_snap = self.y - air_y;
        }
        None
    }

    /// The landing `slope_pass` held back, once this step's update has stopped the rise
    /// (in the ROM, the collision after that update lands).
    fn land_late(&mut self) {
        let Some((y, push)) = self.late_landing.take() else { return };
        if self.vy < 0 && self.slide == 0 {
            return;
        }
        self.y = y;
        self.vy = 0;
        self.on_ground = true;
        self.flutter = 0;
        self.stomp_chain = 0;
        self.push = push;
        // The ROM was standing there already at the end of the last step.
        if let Some(now) = &mut self.now {
            (now.1, now.2) = (y, true);
        }
    }

    /// The ceiling part of 0x0809A7E8: the head's line in its cell against the height
    /// map's ceiling.
    fn ceiling(&mut self, map: &Level, head: u8, head_dy: i32, col: i32, fy: i32) -> Option<(i32, i32)> {
        let at = (fy.div_euclid(SUB) + head_dy) & 0xF;
        let ceiling = map.slope_ceiling(head, col);
        if ceiling < at {
            return None;
        }
        let rising = self.vy < 0;
        if rising {
            if self.in_water {
                self.vy = 0;
            } else {
                self.bonk = true;
            }
        }
        self.y += (ceiling - at) * SUB;
        // The block under the head-centre probe takes the bump.
        let (tx, ty) = (col.div_euclid(TILE_PX), (fy.div_euclid(SUB) + head_dy).div_euclid(TILE_PX));
        (rising && map.at(tx, ty) == TileKind::Solid).then_some((tx, ty))
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
        let (w, h) = (40, 6);
        let mut kinds = vec![TileKind::Empty; w * h];
        for x in 0..w {
            kinds[(h - 1) * w + x] = TileKind::Solid;
        }
        for y in 0..h {
            kinds[y * w + 4] = TileKind::Solid;
        }
        let mut s = Sim::new(Level::from_kinds(w, h, &kinds), 8 * TILE_PX);
        s.run(Buttons::LEFT, 300);
        assert_eq!(s.p.px().0, 5 * TILE_PX);
        assert_eq!(s.p.vx, 0);
    }

    #[test]
    fn pushed_into_a_wall_the_frame_snaps_to_the_16_px_grid() {
        let (w, h) = (40, 6);
        let mut kinds = vec![TileKind::Empty; w * h];
        for x in 0..w {
            kinds[(h - 1) * w + x] = TileKind::Solid;
        }
        for y in 0..h {
            kinds[y * w + 4] = TileKind::Solid;
        }
        let mut s = Sim::new(Level::from_kinds(w, h, &kinds), 6 * TILE_PX);
        s.run(Buttons::empty(), 10);
        // Larry's shell push: speed 0x30 left (3 px a frame), +0x70 = 8.
        s.p.vx = -0x30 * UNIT;
        s.p.push_timer = 8;
        let mut snapped = false;
        for _ in 0..40 {
            s.run(Buttons::empty(), 1);
            if s.p.vx == 0 && s.p.frame().0 < 5 * TILE_PX + 8 {
                snapped = true;
                break;
            }
        }
        assert!(snapped);
        // 0x0809E664: x & ~0xFFF + 0x1000 (pushed right), not the wall's edge (frame x 0x4E).
        assert_eq!(s.p.frame().0, 5 * TILE_PX);
        assert_eq!(s.p.push_timer, 0, "used up");
        // Without the timer: the wall's edge.
        let mut s = Sim::new(Level::from_kinds(w, h, &kinds), 6 * TILE_PX);
        s.run(Buttons::empty(), 10);
        s.p.vx = -0x30 * UNIT;
        s.run(Buttons::empty(), 10);
        assert_eq!(s.p.frame().0, 5 * TILE_PX - 2);
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

    fn hill() -> Level {
        crate::level::test_hill()
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
    fn down_on_a_slope_slides_and_gathers_speed_down_it() {
        // The hill's right side goes down to the right (push 3).
        let mut s = Sim::new(hill(), 13 * TILE_PX + 4);
        assert!(s.p.on_ground && s.p.slide == 0);
        s.run(Buttons::DOWN, 1);
        assert_eq!(s.p.slide, 2, "the push less 1");
        assert_eq!(s.p.pose(), Pose::Slide);
        s.run(Buttons::DOWN, 4);
        assert_eq!((s.p.slide, s.p.vx / UNIT), (10, 10));
        // On the flat it runs down by 1 a frame, Down or not, and stops.
        s.run(Buttons::empty(), 120);
        assert!(s.p.px().0 > 16 * TILE_PX, "slid off the slope");
        assert_eq!((s.p.slide, s.p.vx), (0, 0));
        assert_ne!(s.p.pose(), Pose::Slide);
    }

    #[test]
    fn down_on_flat_ground_does_not_slide_and_left_right_or_up_end_a_slide() {
        let mut s = Sim::new(hill(), 2 * TILE_PX);
        s.run(Buttons::DOWN, 10);
        assert_eq!(s.p.slide, 0);
        for end in [Buttons::LEFT, Buttons::RIGHT, Buttons::UP] {
            let mut s = Sim::new(hill(), 13 * TILE_PX + 4);
            s.run(Buttons::DOWN, 4);
            assert!(s.p.slide > 0);
            s.run(end, 1);
            assert_eq!(s.p.slide, 0, "{end:?}");
        }
    }

    #[test]
    fn big_forms_crouch_with_down_alone_and_keep_it_through_a_jump() {
        let mut s = Sim::new(floor(), 64);
        s.p.set_form(Form::Big);
        s.run(Buttons::empty(), 4);
        let feet = s.p.px().1 + s.p.height();
        s.run(Buttons::DOWN, 2);
        assert!(s.p.crouch);
        assert_eq!((s.p.height(), s.p.px().1 + s.p.height()), (16, feet), "small box, feet kept");
        assert_eq!(s.p.pose(), Pose::Crouch);
        s.run(Buttons::DOWN | Buttons::A, 1);
        s.run(Buttons::empty(), 10);
        assert!(!s.p.on_ground && s.p.crouch, "crouched through the jump");
        s.run(Buttons::empty(), 60);
        assert!(s.p.on_ground && !s.p.crouch, "stands on landing without Down");
        s.run(Buttons::DOWN | Buttons::RIGHT, 4);
        assert!(!s.p.crouch, "not with Left or Right");
        let mut small = Sim::new(floor(), 64);
        small.run(Buttons::DOWN, 4);
        assert!(!small.p.crouch);
    }

    #[test]
    fn crouched_on_a_lift_under_a_low_ceiling_the_player_stays_down() {
        let (w, h) = (40, 6);
        let mut kinds = vec![TileKind::Empty; w * h];
        for x in 0..w {
            kinds[(h - 1) * w + x] = TileKind::Solid;
        }
        // Ground over the crouching head: frame (8, 0x0A) is in row 3 at column 4.
        kinds[3 * w + 4] = TileKind::Solid;
        let map = Level::from_kinds(w, h, &kinds);
        for riding in [Some(0), None] {
            let p = Player::spawn(4 * TILE_PX + 2, 5 * TILE_PX - Form::Small.height());
            let mut s = Sim { p, map: map.clone(), frame: 0, held: Buttons::empty() };
            s.p.set_form(Form::Big);
            s.run(Buttons::empty(), 4);
            assert!(s.p.on_ground);
            s.run(Buttons::DOWN, 4);
            assert!(s.p.crouch);
            s.p.riding = riding;
            s.run(Buttons::empty(), 4);
            assert_eq!(s.p.crouch, riding.is_some(), "riding {riding:?}");
        }
    }

    #[test]
    fn the_level_left_end_stops_the_frame_at_x_4() {
        // As in mGBA (1-1 / 1-2): the frame stops at x 4 (fraction kept) and Left no longer
        // moves him; Right does.
        let mut s = Sim::new(floor(), 40);
        s.run(Buttons::LEFT | Buttons::B, 60);
        let fx = s.p.frame().0;
        assert_eq!((fx, s.p.vx), (LEFT_END, 0));
        let x = s.p.x;
        s.run(Buttons::LEFT, 30);
        assert_eq!(s.p.x, x, "held at the end");
        s.run(Buttons::RIGHT, 10);
        assert!(s.p.x > x);
    }

    #[test]
    fn a_jump_from_a_slope_leaves_from_where_the_move_put_the_player() {
        // Walking down the hill, the collision follows the slope down; the ROM jumps before
        // that collision (checked in mGBA, World 1-5), so the jump starts from the higher y.
        let mut s = Sim::new(hill(), 14 * TILE_PX);
        s.run(Buttons::RIGHT, 6);
        let y = s.p.y - s.p.floor_snap;
        s.run(Buttons::RIGHT | Buttons::A, 1);
        assert_eq!(s.p.y, y + s.p.vy);
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

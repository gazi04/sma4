//! The title screen (mode 1 sub-state 9, 0x0802E2C0) the game-over menu's QUIT goes back
//! to: the curtain rises, Mario and Luigi play their scene on the stage, the logo drops,
//! then the title waits with Koopas walking by; A or Start opens the file select and the
//! mode select, which start a new game. See docs/re-notes.md, "Title screen".
//!
//! The screen is mostly palette and BG work, so this module keeps the GBA's video state
//! the way the ROM drives it (`Video`: VRAM, palette, OAM and the registers the VBlank
//! handler 0x08000EA4 latches from their RAM shadows) and the host draws that. Graphics,
//! maps, palettes and the ROM tables come from `sma4_assets::TitleAsset`; table reads cite
//! their ROM address.
//!
//! Per frame (0x0802E2C0): the OAM buffer is cleared (0x0800295C), then the step
//! 0x08140CC4[0x03003930]:
//! * 0 (0x0802BE9C): black (BLDY 0x10), BG/OBJ set up, the state block cleared;
//! * 1 (0x0802C044): the curtain (BG1), scenery (BG2, 0x20 lower), floor (BG3), palettes;
//! * 2 (0x0802E1FC): BLDY back to 0, a step a frame;
//! * 3 (0x0802C0EC): 0x36 frames (A / Start: straight to the title);
//! * 4 (0x0802E234): the Koopas' spawn timer, then the phase 0x08140C74[+0xDB].
//!
//! The first two steps unpack whole screens and run over several frames on the GBA
//! (`SETUP_FRAMES`, measured on the running ROM); the screen is black meanwhile.

use std::sync::Arc;

use sma4_assets::{TITLE_PLAYER_BLOCK, TitleAsset, title_player_block};

use crate::input::Buttons;

pub const VRAM_LEN: usize = 0x1_8000;
/// OAM buffer 0x03002DC8-0x03003347; the VBlank copies its first 0x400 bytes.
const OAM_BUF: usize = 0x580;
const OAM_LEN: usize = 0x400;
/// Palette buffers 0x02000000-0x02000FFF (BG 0x000, OBJ 0x200, the logo fade's target
/// 0x820 and step masks 0xC20), as halfwords.
const PAL_BUF: usize = 0x800;
const PAL_OBJ: usize = 0x100;
const PAL_TARGET: usize = 0x410;
const PAL_MASKS: usize = 0x610;

/// Frames the setup steps take on the GBA (0x0802BE9C unpacks the title's graphics,
/// 0x0802C044 its maps): read from the running ROM, the step's state showing up that many
/// frames after it started.
const SETUP_FRAMES: [u8; 2] = [9, 6];

// Buttons as 0x03002DBE holds them.
const KEY_A: u16 = 1;
const KEY_B: u16 = 2;
const KEY_START: u16 = 8;
const KEY_UP: u16 = 0x40;
const KEY_DOWN: u16 = 0x80;

/// Register values 0x0802BE9C sets.
const DISPCNT: u16 = 0x1E00;
const BG_CNT: [u16; 4] = [0xD400, 0xD801, 0xDC02, 0x0F03];
/// BLDCNT: everything darkened by BLDY (0xFF); the erase prompt darkens BG3 and the
/// backdrop (0xE8, 0x0802CB90).
const BLD_ALL: u16 = 0xFF;
const BLD_ERASE: u16 = 0xE8;
const BLACK: u16 = 0x10;

// LZ77 maps (`sma4_assets::TITLE_LZ`).
const MAP_CURTAIN: u32 = 0x0821_F8C0;
const MAP_STAGE: u32 = 0x0822_1040;
const MAP_SCENERY: u32 = 0x0822_B114;
const MAP_FLOOR: u32 = 0x0823_ADD0;
const MAP_FILES: u32 = 0x0821_FCB8;
const MAP_MODES: u32 = 0x0822_0980;

// Palettes.
const PAL_BG: u32 = 0x0821_DB80;
const PAL_BG_8: u32 = 0x0821_ECE0;
const PAL_LOGO: u32 = 0x0821_DC00;
const PAL_OBJ_0: u32 = 0x0821_E380;
const PAL_OBJ_5: u32 = 0x0821_E780;
const PAL_OBJ_7: u32 = 0x0821_E420;
const PAL_OBJ_D: u32 = 0x0821_EAA0;
/// Colour curves 0x08007B5C uses, BG and OBJ: the first of each (0x0300378B = 0, as for all
/// the port's extracted palettes; a new save has 2, the second curve, slightly brighter).
const CURVE_BG: u32 = 0x0813_2DE4;
const CURVE_OBJ: u32 = 0x0813_2EE4;

/// The port's assets are English (0x03003777 = 0).
const LANGUAGE: u32 = 0;

/// What the title asks of the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleEvent {
    /// 0x0802C044 sets the first of the random bytes (0x03003878).
    Seed(u8),
    /// The file and mode are chosen and the screen has faded out (0x0802DF94): a new game
    /// for `players` players.
    Start { players: u8 },
}

/// The GBA's video state as the VBlank handler 0x08000EA4 leaves it.
#[derive(Debug, Clone)]
pub struct Video {
    pub vram: Vec<u8>,
    pub palette: [u16; 512],
    pub oam: [u8; OAM_LEN],
    pub dispcnt: u16,
    pub bgcnt: [u16; 4],
    /// BG0-3 (x, y) scroll. BG0 and BG1 share 0x03003750 / 54.
    pub scroll: [(u16, u16); 4],
    pub bldcnt: u16,
    pub bldalpha: u16,
    pub bldy: u16,
}

/// A player of the scene (0x30 bytes at +0x00 / +0x30).
#[derive(Debug, Clone, Copy, Default)]
struct Actor {
    /// +0x00 / +0x04: screen x, and y (0xA0 standing on the floor).
    x: u16,
    y: u16,
    /// +0x08 / +0x09: fractions.
    xs: u8,
    ys: u8,
    /// +0x0A / +0x0B: speeds, 1/16 px a frame (signed).
    vx: u8,
    vy: u8,
    /// +0x0C: in the air.
    air: u8,
    /// +0x0D: 0x40 facing right (drawn flipped).
    face: u8,
    /// +0x0E: moving left (1) or right (2).
    dir: u8,
    /// +0x0F / +0x10: walk frame and its timer.
    frame: u8,
    frame_t: u8,
    /// +0x11: pose, +0x12: form (0 small, 1 big, 2 raccoon).
    pose: u8,
    form: u8,
    /// +0x13: OAM slot * 4.
    oam: u8,
    /// +0x14: off-screen sides.
    edge: u8,
    /// +0x15: tail wag; +0x16: holding a shell; +0x17: kick / turn; +0x18: grab.
    wag: u8,
    hold: u8,
    kick: u8,
    grab: u8,
    /// +0x19 / +0x1A: scripted buttons held and newly pressed (1 right, 2 left, 4 down,
    /// 8 up, 0x10 grab, 0x20 hold, 0x40 run, 0x80 jump).
    held: u8,
    pressed: u8,
    /// The pose and form the tile pointers (+0x1C-+0x2C) were last set from (0x08035AB0).
    tiles: Option<(u8, u8)>,
}

/// A prop of the scene, later a walking Koopa (0x14 bytes from +0x60).
#[derive(Debug, Clone, Copy, Default)]
struct Prop {
    /// +0x60 / +0x64.
    x: u16,
    y: u16,
    /// +0x68 / +0x69: speeds; +0x6A / +0x6B: fractions.
    vx: u8,
    vy: u8,
    xs: u8,
    ys: u8,
    /// +0x6C: OAM flips (0x40 h, 0x80 v).
    flip: u8,
    /// +0x6D: 0 off, 1 starting, 2 and up running.
    state: u8,
    /// +0x6E / +0x6F / +0x70.
    timer: u8,
    var: u8,
    frame: u8,
}

#[derive(Debug, Clone)]
pub struct Title {
    rom: Arc<TitleAsset>,
    pub video: Video,
    // RAM the VBlank reads.
    pal: Vec<u16>,
    /// 0x03003782: bit 0 upload the BG palette, bit 1 the OBJ palette.
    pal_upload: u8,
    oam: [u8; OAM_BUF],
    /// 0x03003781: next OAM slot for sprite lists (0x0803A1BC).
    oam_next: u8,
    dispcnt: u16,
    bgcnt: [u16; 4],
    bldcnt: u16,
    bldalpha: u16,
    bldy: u16,
    /// 0x03003750 / 54 (BG0 and BG1), 58 / 5C (BG2), 60 / 64 (BG3).
    bg1: (u16, u16),
    bg2: (u16, u16),
    bg3: (u16, u16),
    /// 0x03003AE8: frames, counted in VBlank.
    pub frame: u8,
    /// 0x03003930 and the frames its setup steps still take.
    step: u8,
    busy: u8,
    /// Sub-state 8 (0x0800373C) after the title timed out: steps done and the frame
    /// counter 0x03003784.
    fade: Option<(u8, u8)>,
    // The state block 0x03007828.
    p: [Actor; 2],
    props: [Prop; 6],
    /// +0xD8: Mario shrinking; +0xD9: the walk-in under the curtain; +0xDA: the actor
    /// being run; +0xDB: the phase.
    shrink: u8,
    walk_in: u8,
    current: u8,
    pub phase: u8,
    /// +0xDC / +0xDD: the title's timeout (0x20 * 0x60 frames); +0xDE: timed out.
    timeout: (u8, u8),
    timed_out: u8,
    /// +0xE0: a timer the events and the palette cycle share.
    timer: u8,
    /// +0xE1 / +0xE2: script positions; +0xE3 / +0xE4: frames left of the entries;
    /// +0xE5 / +0xE6: their buttons.
    script: [u8; 2],
    script_t: [u8; 2],
    script_keys: [u8; 2],
    /// +0xE7 / +0xE8: the scene's event and its counter.
    event: u8,
    event_n: u8,
    /// +0xE9 / +0xEA: the Koopas still to start, and the frames to the next.
    spawn: u8,
    spawn_t: u8,
    /// +0xEB / +0xEC: the logo's palette cycle on, and its step.
    cycle: u8,
    cycle_n: u8,
    /// +0xEE: the fraction the actors' speed changes add up in.
    accel: u8,
    /// +0xEF: the puff of Mario turning raccoon; +0xF0: frames the walk-in waits.
    puff: u8,
    walk_wait: u8,
    /// +0xF1: the menus' step; +0xF5 the erase prompt's; +0xF9 / +0xFA its sign's frame
    /// and timer.
    menu: u8,
    erase: u8,
    msg: u8,
    msg_t: u8,
    /// 0x03003B90 / 0x03003B92: the logo fade's direction and frame (0x08008E98).
    logo: (u8, u8),
    /// 0x03003931: the file cursor (0-2 files, 4 erase); 0x0300385E: the mode (0 Mario,
    /// 1 Mario & Luigi), then the player count.
    cursor: u8,
    mode: u8,
    /// 0x03003A64: the save files (0x18: new; the port keeps no saves).
    files: [u8; 3],
    /// 0x03002A56: the file used last.
    last_file: u8,
    /// Frames phase 11 has waited.
    start_wait: u8,
}

/// Frames phase 11 waits for the start sound (0x080E7EB0(0x21)).
const START_SOUND_FRAMES: u8 = 15;

/// Save file state of a new file (0x0803E79C).
const FILE_NEW: u8 = 0x18;

impl Title {
    /// The title as sub-state 9 starts it (0x03003930 = 0).
    pub fn new(rom: Arc<TitleAsset>) -> Self {
        let mut vram = rom.vram.clone();
        vram.resize(VRAM_LEN, 0);
        Self {
            video: Video {
                vram,
                palette: [0; 512],
                oam: std::array::from_fn(|i| if i % 8 == 0 { 0xA0 } else { 0 }),
                dispcnt: 0,
                bgcnt: [0; 4],
                scroll: [(0, 0); 4],
                bldcnt: 0,
                bldalpha: 0,
                bldy: 0,
            },
            rom,
            pal: vec![0; PAL_BUF],
            pal_upload: 0,
            // As the fade before the title left it (0x0800295C).
            oam: std::array::from_fn(|i| if i % 8 == 0 { 0xA0 } else { 0 }),
            oam_next: 0,
            dispcnt: 0,
            bgcnt: [0; 4],
            bldcnt: 0,
            bldalpha: 0,
            bldy: 0,
            bg1: (0, 0),
            bg2: (0, 0),
            bg3: (0, 0),
            frame: 0,
            step: 0,
            busy: 0,
            fade: None,
            p: [Actor::default(); 2],
            props: [Prop::default(); 6],
            shrink: 0,
            walk_in: 0,
            current: 0,
            phase: 0,
            timeout: (0, 0),
            timed_out: 0,
            timer: 0,
            script: [0; 2],
            script_t: [0; 2],
            script_keys: [0; 2],
            event: 0,
            event_n: 0,
            spawn: 0,
            spawn_t: 0,
            cycle: 0,
            cycle_n: 0,
            accel: 0,
            puff: 0,
            walk_wait: 0,
            menu: 0,
            erase: 0,
            msg: 0,
            msg_t: 0,
            logo: (0, 0),
            cursor: 0,
            mode: 0,
            files: [FILE_NEW; 3],
            last_file: 0,
            start_wait: 0,
        }
    }

    fn r8(&self, a: u32) -> u8 {
        self.rom.rom_u8(a)
    }

    fn r16(&self, a: u32) -> u16 {
        self.rom.rom_u16(a)
    }

    fn r32(&self, a: u32) -> u32 {
        self.rom.rom_u32(a)
    }

    /// One frame with the buttons newly pressed.
    pub fn step(&mut self, pressed: Buttons) -> Option<TitleEvent> {
        self.vblank();
        let keys = pressed.bits();
        if let Some((done, counter)) = self.fade {
            self.fade = self.fade_step(done, counter);
            if self.fade.is_none() {
                // Sub-state 9 again, from its first step.
                self.step = 0;
                self.busy = 0;
            }
            return None;
        }
        if self.busy > 0 {
            self.busy -= 1;
            return None;
        }
        // 0x0802E2C0.
        self.clear_oam();
        match self.step {
            0 => self.setup(),
            1 => {
                self.setup_screen();
                return Some(TitleEvent::Seed(0x88));
            }
            2 => {
                // 0x0802E1FC
                self.bldy -= 1;
                if self.bldy == 0 {
                    self.step += 1;
                    self.bldcnt = 0;
                }
            }
            3 => {
                // 0x0802C0EC
                if keys & (KEY_START | KEY_A) == 0 {
                    self.timer = self.timer.wrapping_sub(1);
                    if self.timer as i8 >= 0 {
                        return None;
                    }
                } else if self.phase != 6 {
                    self.phase = 6;
                }
                self.step += 1;
            }
            _ => return self.run(keys),
        }
        None
    }

    /// 0x08000EA4: the frame counter, OAM, the palettes flagged, the players' tiles
    /// (0x08035B88), the registers and scroll.
    fn vblank(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.video.oam.copy_from_slice(&self.oam[..OAM_LEN]);
        if self.pal_upload & 1 != 0 {
            self.video.palette[..PAL_OBJ].copy_from_slice(&self.pal[..PAL_OBJ]);
        }
        if self.pal_upload & 2 != 0 {
            self.video.palette[PAL_OBJ..].copy_from_slice(&self.pal[PAL_OBJ..2 * PAL_OBJ]);
        }
        self.pal_upload = 0;
        self.player_dma();
        let v = &mut self.video;
        v.dispcnt = self.dispcnt;
        v.bgcnt = self.bgcnt;
        v.bldcnt = self.bldcnt;
        v.bldalpha = self.bldalpha;
        v.bldy = self.bldy;
        v.scroll = [self.bg1, self.bg1, self.bg2, self.bg3];
    }

    /// 0x08035B88: four rows of 8 tiles cleared at 0x06011000, then each player's tiles.
    fn player_dma(&mut self) {
        for row in 0..4 {
            let at = 0x1_1000 + row * 0x400;
            self.video.vram[at..at + 0x100].fill(0);
            for (i, p) in self.p.iter().enumerate() {
                let Some((pose, form)) = p.tiles else { continue };
                let k = title_player_block(i, form as usize % 3, pose as usize % 32);
                if let Some(b) = self.rom.players.get(k).filter(|b| b.len() == TITLE_PLAYER_BLOCK) {
                    self.video.vram[at + i * 0x80..at + i * 0x80 + 0x80].copy_from_slice(&b[row * 0x80..][..0x80]);
                }
            }
        }
    }

    /// 0x0800295C: every OAM entry hidden (y 0xA0), all else 0.
    fn clear_oam(&mut self) {
        for e in self.oam.chunks_mut(8) {
            e.copy_from_slice(&[0xA0, 0, 0, 0, 0, 0, 0, 0]);
        }
    }

    /// Step 0, 0x0802BE9C: black, the registers, the state block cleared (0x0802AC70), the
    /// graphics loaded (0x0802AE30: the VRAM of `TitleAsset`; the maps stay until step 1),
    /// the scroll zeroed (0x08009F38).
    fn setup(&mut self) {
        let old = std::mem::replace(self, Title::new(self.rom.clone()));
        let maps = 0x7800..0x1_0000;
        self.video.vram[maps.clone()].copy_from_slice(&old.video.vram[maps]);
        self.video.palette = old.video.palette;
        self.video.oam = old.video.oam;
        // RAM outside the state block.
        self.pal = old.pal;
        self.pal_upload = old.pal_upload;
        self.frame = old.frame;
        self.logo = old.logo;
        self.cursor = old.cursor;
        self.mode = old.mode;
        self.files = old.files;
        self.last_file = old.last_file;
        self.bldy = BLACK;
        self.bldcnt = BLD_ALL;
        self.dispcnt = DISPCNT;
        self.bgcnt = BG_CNT;
        self.step = 1;
        self.busy = SETUP_FRAMES[0] - 1;
    }

    /// Step 1, 0x0802C044.
    fn setup_screen(&mut self) {
        self.spawn = 0xFF;
        for p in &mut self.p {
            p.y = p.y & 0xFF00 | 0xA0;
            p.x = p.x & 0xFF00 | 0xF0;
            p.form = 1;
        }
        self.load_map(MAP_SCENERY, 2);
        self.bg2.1 = 0x20;
        self.load_map(MAP_CURTAIN, 1);
        self.load_bg_palette();
        self.load_obj_palette();
        self.load_map(MAP_FLOOR, 3);
        self.timer = 0x35;
        self.step = 2;
        self.busy = SETUP_FRAMES[1] - 1;
    }

    /// The BG map at `src` unpacked to BG `bg`'s screen base (0x080EAA9C).
    fn load_map(&mut self, src: u32, bg: usize) {
        let base = (self.bgcnt[bg] as usize & 0x1F00) * 8;
        let rom = self.rom.clone();
        if let Some(d) = rom.lz(src) {
            let end = (base + d.len()).min(VRAM_LEN);
            self.video.vram[base..end].copy_from_slice(&d[..end - base]);
        }
    }

    fn map_write(&mut self, at: usize, v: u16) {
        if let Some(b) = self.video.vram.get_mut(at..at + 2) {
            b.copy_from_slice(&v.to_le_bytes());
        }
    }

    /// BG1's screen base in VRAM.
    fn bg1_map(&self) -> usize {
        (self.bgcnt[1] as usize & 0x1F00) * 8
    }

    /// 0x08007B5C with the port's colour setting.
    fn colour(&self, c: u16, obj: bool) -> u16 {
        let curve = if obj { CURVE_OBJ } else { CURVE_BG };
        let ch = |v: u16| self.r16(curve + (v & 0x1F) as u32 * 2);
        ch(c) | ch(c >> 5) << 5 | ch(c >> 10) << 10
    }

    /// `n` colours from the ROM at `src` into the palette buffer from `at`.
    fn copy_colours(&mut self, src: u32, at: usize, n: usize) {
        for i in 0..n {
            self.pal[at + i] = self.r16(src + i as u32 * 2);
        }
    }

    fn convert(&mut self, at: usize, n: usize, obj: bool) {
        for i in at..at + n {
            self.pal[i] = self.colour(self.pal[i], obj);
        }
    }

    /// 0x0802BE20: BG palettes 0-3 and 8-10, colours 0 of 0, 1, 3 and 5 black.
    fn load_bg_palette(&mut self) {
        self.pal[..2 * PAL_OBJ].fill(0);
        self.copy_colours(PAL_BG, 0, 0x40);
        self.copy_colours(PAL_BG_8, 0x80, 0x30);
        for i in [0, 8, 0x1F, 0x2F] {
            self.pal[i] = 0;
        }
        self.convert(0, 0x80, false);
        self.pal_upload = 1;
    }

    /// 0x08008DCC: OBJ palettes 0-4, 5, 7 and 0xD.
    fn load_obj_palette(&mut self) {
        let o = PAL_OBJ;
        self.copy_colours(PAL_OBJ_0, o, 0x50);
        self.copy_colours(PAL_OBJ_5, o + 0x50, 0x20);
        self.copy_colours(PAL_OBJ_7, o + 0x70, 0x20);
        self.copy_colours(PAL_OBJ_D, o + 0xD0, 0x20);
        self.convert(o, 0x80, true);
        self.pal_upload |= 2;
    }

    /// 0x08008E50: BG palettes 0-7.
    fn load_stage_palette(&mut self) {
        self.copy_colours(PAL_BG, 0, 0x80);
        self.convert(0, 0x80, false);
        self.pal_upload = 1;
    }

    /// 0x0800373C, the fade of sub-state 8: every other frame 4 off each colour channel,
    /// 8 times. None once done.
    fn fade_step(&mut self, done: u8, counter: u8) -> Option<(u8, u8)> {
        let counter = counter.wrapping_sub(1);
        if (counter as i8) >= 0 {
            return Some((done, counter));
        }
        for c in &mut self.pal[..2 * PAL_OBJ] {
            let r = (*c & 0x1F).saturating_sub(4);
            let g = (*c & 0x3E0).saturating_sub(0x80);
            let b = (*c & 0x7C00).saturating_sub(0x1000);
            *c = b | g | r;
        }
        self.pal_upload = 3;
        if done + 1 > 7 {
            self.clear_oam();
            return None;
        }
        Some((done + 1, 1))
    }

    /// Step 4, 0x0802E234.
    fn run(&mut self, keys: u16) -> Option<TitleEvent> {
        if self.spawn < 6 {
            self.spawn_t = self.spawn_t.wrapping_sub(1);
            if self.spawn_t == 0 {
                self.spawn_t = self.r8(0x0814_0CBE + self.spawn as u32);
                let k = self.r8(0x0814_0CB8 + self.spawn as u32) as usize;
                self.props[k].state = 1;
                self.spawn += 1;
            }
        }
        // 0x0802E1BC.
        self.oam_next = 0;
        self.p[1].pressed = 0;
        self.p[0].pressed = 0;
        let event = match self.phase {
            0 => {
                self.curtain(keys);
                None
            }
            1 => {
                self.scene(keys);
                None
            }
            2 => {
                self.end_scene();
                None
            }
            3 => {
                self.idle(keys);
                None
            }
            4 => {
                // 0x0802C7D0: the mode becomes the player count.
                self.koopas();
                self.mode += 1;
                self.last_file = self.cursor;
                self.phase = 9;
                None
            }
            6 => {
                self.skip();
                None
            }
            7 => {
                self.files_screen(keys);
                None
            }
            8 => {
                // 0x0802DEEC
                self.koopas();
                if self.menu == 0 {
                    self.modes_setup();
                } else {
                    self.modes_input(keys);
                }
                None
            }
            9 => {
                // 0x0802DF14: the music fades.
                self.koopas();
                self.bldy = 0;
                self.bldcnt = BLD_ALL;
                self.phase += 1;
                None
            }
            10 => {
                // 0x0802DF60
                self.koopas();
                self.bldy += 1;
                if self.bldy == BLACK {
                    self.phase += 1;
                }
                None
            }
            11 => {
                // 0x0802DF94: once the start sound 0x21 is over (`START_SOUND_FRAMES`, read
                // from the running ROM) the file is loaded and the game starts.
                self.start_wait += 1;
                (self.start_wait >= START_SOUND_FRAMES).then_some(TitleEvent::Start { players: self.mode })
            }
            15 => {
                // 0x0802E12C: B on the menus, the screen darkens to the title.
                self.koopas();
                if self.bldy == 0 {
                    self.bldcnt = BLD_ALL;
                }
                self.bldy += 1;
                if self.bldy == BLACK {
                    self.load_map(MAP_STAGE, 1);
                    self.bg1.0 = 0;
                    self.phase += 1;
                }
                None
            }
            16 => {
                // 0x0802E18C
                self.koopas();
                self.bldy -= 1;
                if self.bldy == 0 {
                    self.phase = 3;
                }
                None
            }
            _ => None,
        };
        if self.timed_out != 0 {
            // Sub-state 8, then the title from the start.
            self.fade = Some((0, 0));
        }
        event
    }

    /// Start or A in the scene: sound 0x21, straight to the title.
    fn skip_on(&mut self, keys: u16) {
        if keys & (KEY_START | KEY_A) != 0 && self.phase != 6 {
            self.phase = 6;
        }
    }

    /// Phase 0, 0x0802C360: the curtain rises 2 px a frame; from 0x26 Mario and Luigi walk
    /// in place underneath.
    fn curtain(&mut self, keys: u16) {
        self.bg1.1 = self.bg1.1.wrapping_add(2);
        if self.bg1.1 == 0xA8 {
            self.event = 2;
            self.event_n = 2;
            self.phase += 1;
            self.walk_in = 0;
            self.actors();
            return;
        }
        if self.bg1.1 == 0x26 {
            self.walk_wait = 0x40;
            self.walk_in = 0x40;
        }
        if self.walk_in != 0 {
            self.actors();
        }
        self.skip_on(keys);
    }

    /// Phase 1, 0x0802C5A0: the scripts drive the players.
    fn scene(&mut self, keys: u16) {
        self.script_t[0] = self.script_t[0].wrapping_sub(1);
        if self.script_t[0] == 0xFF {
            self.script_mario();
        }
        self.script_t[1] = self.script_t[1].wrapping_sub(1);
        if self.script_t[1] == 0xFF {
            self.script_luigi();
        }
        self.p[0].held = self.script_keys[0];
        self.p[1].held = self.script_keys[1];
        self.scene_event();
        self.actors();
        if self.cycle != 0 {
            self.palette_cycle();
        }
        self.skip_on(keys);
    }

    /// 0x0802C4B8: Mario's next (frames, buttons) at 0x08142600; 0xFF: an event, 0xFE:
    /// the scene's end.
    fn script_mario(&mut self) {
        self.p[0].pressed = 0;
        self.script_keys[0] = 0;
        let at = 0x0814_2600 + self.script[0] as u32;
        match self.r8(at) {
            0xFF => {
                self.event = self.r8(at + 1);
                self.script[0] = self.script[0].wrapping_add(2);
                self.script_t[0] = 0;
            }
            0xFE => {
                self.phase += 1;
                self.script[0] = 0;
            }
            n => {
                self.script_t[0] = n;
                let k = self.r8(at + 1);
                self.p[0].pressed = k;
                self.script_keys[0] = k;
                self.script[0] = self.script[0].wrapping_add(2);
            }
        }
    }

    /// 0x0802C540: Luigi's, at 0x08142658 (0xFF: from the start).
    fn script_luigi(&mut self) {
        self.p[1].pressed = 0;
        self.script_keys[1] = 0;
        let at = 0x0814_2658 + self.script[1] as u32;
        match self.r8(at) {
            0xFF => self.script[1] = 0,
            n => {
                self.script_t[1] = n;
                let k = self.r8(at + 1);
                self.p[1].pressed = k;
                self.script_keys[1] = k;
                self.script[1] = self.script[1].wrapping_add(2);
            }
        }
    }

    /// 0x0802C33C: the scene's event 0x0814087C[+0xE7].
    fn scene_event(&mut self) {
        match self.event {
            2 => {
                // 0x0802C16C: the stage behind the curtain, colours 4-9 black.
                self.event_n = self.event_n.wrapping_add(1);
                if self.event_n == 0x1D {
                    self.event = 0;
                    self.load_map(MAP_STAGE, 1);
                    self.pal[4..10].fill(0);
                    self.pal_upload = 1;
                }
            }
            3 => {
                // 0x0802C1C8: the logo drops 4 px a frame.
                self.bg1.1 = self.bg1.1.wrapping_sub(4);
                if self.bg1.1 == 0 {
                    self.timer = 0x10;
                    self.event += 1;
                }
            }
            4 => {
                // 0x0802C200 / 0x0802C228: and shakes, 0x10 times.
                self.bg1.1 = self.bg1.1.wrapping_sub(2);
                self.event += 1;
            }
            5 => {
                self.bg1.1 = self.bg1.1.wrapping_add(2);
                self.event -= 1;
                self.timer = self.timer.wrapping_sub(1);
                if self.timer == 0 {
                    self.event += 2;
                    self.event_n = 3;
                    self.timer = 2;
                }
            }
            6 => {
                // 0x0802C284: the Koopas start, the logo fades in (0x08008D20), BG2 up.
                self.timer = self.timer.wrapping_sub(1);
                if (self.timer as i8) < 0 {
                    self.spawn = 0;
                    self.spawn_t = 1;
                    self.event += 1;
                    self.timer = 0x16;
                    self.logo_fade_start();
                    self.logo.0 = 0;
                    self.bg2.1 = 0;
                }
            }
            7 => {
                // 0x0802C2EC
                self.logo_fade();
                if self.logo.0 == 2 {
                    self.event = 0;
                    self.cycle = 1;
                }
            }
            8 => self.event += 1,
            _ => {}
        }
    }

    /// 0x08008D20: the logo's colours (BG 0x0821DB80 / 0x0821DC00) become the fade's
    /// target, palettes 0 and 4-7 black.
    fn logo_fade_start(&mut self) {
        self.copy_colours(PAL_BG, PAL_TARGET, 0x10);
        self.copy_colours(PAL_LOGO, PAL_TARGET + 0x40, 0x40);
        for i in 0..0x10 {
            self.pal[i] = 0;
            self.pal[0x40 + i] = 0;
            self.pal[0x50 + i] = 0;
            self.pal[0x60 + i] = 0;
            self.pal[0x70 + i] = 0;
        }
        self.pal[PAL_TARGET + 0x1F] = 0x1E78;
        self.pal[PAL_TARGET + 0x2F] = 0x1A5D;
        self.convert(PAL_TARGET, 0x80, false);
        self.pal_upload = 1;
    }

    /// 0x08008E98(0): one frame of the logo's fade. Each channel value steps on the frames
    /// its bit in this frame's mask table says (0x081330E8 / 0x08133128, masked by
    /// 0x08133190[frame & 0xF]); the step is 0x08133188[direction]'s entry for the
    /// channels that move. Colours 0, 0x41-0x7F (not 0 of a palette), then 1-0xF, 0x1F, 0x2F
    /// (0x08009068).
    fn logo_fade(&mut self) {
        let (dir, k) = self.logo;
        let steps = self.r32(0x0813_3188 + (dir as u32 >> 1) * 4);
        let mask = self.r16(0x0813_3190 + (k as u32 & 0xF) * 2);
        let half = (k > 0xF) as u32;
        let (a, b) = (0x0813_30E8 + half * 2, 0x0813_3128 + half * 2);
        for i in 0..4u32 {
            for (j, src) in [(0, a), (4, b)] {
                for q in 0..4u32 {
                    self.pal[PAL_MASKS + (j + q as usize) * 4 + i as usize] = self.r16(src + (i * 2 + q * 8) * 2) & mask;
                }
            }
        }
        let mut i = 0usize;
        loop {
            self.logo_fade_colour(i, steps);
            i += 1;
            if i & 0x7FC0 == 0 {
                i = 0x40;
            }
            if 0x7F80 & i != 0 {
                break;
            }
            if i & 0xF == 0 {
                i += 1;
            }
            if i >= 0x101 {
                break;
            }
        }
        self.logo.1 += 1;
        if self.logo.1 == 0x20 {
            self.logo.0 ^= 2;
            self.logo.1 = 0;
        }
        for i in (1..0x10).chain([0x1F, 0x2F]) {
            self.logo_fade_colour(i, steps);
        }
        self.pal_upload = 3;
    }

    fn logo_fade_colour(&mut self, i: usize, steps: u32) {
        let c = self.pal[PAL_TARGET + i];
        let still = |v: u16| self.pal[PAL_MASKS + (v & 0x1F) as usize] == 0;
        let flags = still(c) as u32 | (still(c >> 5) as u32) << 1 | (still(c >> 10) as u32) << 2;
        self.pal[i] = self.pal[i].wrapping_add(self.r16(steps + flags * 2));
    }

    /// 0x0802C410: every 5 frames the next of the logo's 8 colour sets (0x081425C0) into
    /// colours 0x13-0x16.
    fn palette_cycle(&mut self) {
        self.timer = self.timer.wrapping_sub(1);
        if (self.timer as i8) < 0 {
            self.timer = 4;
            let at = 0x0814_25C0 + (self.cycle_n as u32 & 7) * 8;
            for k in 0..4 {
                self.pal[0x13 + k] = self.colour(self.r16(at + k as u32 * 2), false);
            }
            self.pal_upload = 1;
            self.cycle_n = self.cycle_n.wrapping_add(1);
        }
    }

    /// Phase 2, 0x0802C64C: the props go, the first Koopa starts, the timeout starts.
    fn end_scene(&mut self) {
        for k in 1..5 {
            self.props[k].state = 0;
        }
        self.props[0].state = 1;
        self.timeout = (0x20, 0x60);
        self.phase += 1;
    }

    /// Phase 6, 0x0802C840: the scene skipped, the stage as at its end.
    fn skip(&mut self) {
        self.bg1.1 = 0;
        self.bg2.1 = 0;
        self.load_map(MAP_STAGE, 1);
        self.load_stage_palette();
        self.load_map(MAP_SCENERY, 2);
        self.phase = 2;
    }

    /// Phase 3, 0x0802C72C: the title. A or Start opens the file select.
    fn idle(&mut self, keys: u16) {
        self.timeout.1 = self.timeout.1.wrapping_sub(1);
        if self.timeout.1 == 0 {
            self.timeout.1 = 0x60;
            self.timeout.0 = self.timeout.0.wrapping_sub(1);
            if self.timeout.0 == 0 {
                self.timed_out = 0xFF;
            }
        }
        self.koopas();
        self.copyright();
        self.palette_cycle();
        if keys & (KEY_START | KEY_A) != 0 {
            self.phase = 7;
            self.menu = 0;
            self.cursor = self.last_file;
        }
    }

    /// 0x0802C6B0: "(c) 1988-2003 Nintendo" from OAM slot 0.
    fn copyright(&mut self) {
        let mut slot = 0;
        let mut x = 0x2Eu16;
        for k in 0..11u32 {
            let tile = self.r16(0x0814_08A4 + k * 2);
            if tile != 0 {
                self.oam_set(slot, 0x96, x, tile | 0x2000);
                slot += 1;
            }
            x += 8;
        }
        self.oam_set(slot, 0x4092, 0x808E, 0x2115);
        self.oam_set(slot + 1, 0x4092, 0x80A6, 0x2118);
    }

    fn oam_set(&mut self, slot: usize, a0: u16, a1: u16, a2: u16) {
        let e = &mut self.oam[slot * 8..slot * 8 + 6];
        e[0..2].copy_from_slice(&a0.to_le_bytes());
        e[2..4].copy_from_slice(&a1.to_le_bytes());
        e[4..6].copy_from_slice(&a2.to_le_bytes());
    }

    fn oam16(&self, at: usize) -> u16 {
        u16::from_le_bytes([self.oam[at], self.oam[at + 1]])
    }

    fn set_oam16(&mut self, at: usize, v: u16) {
        self.oam[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }

    /// 0x0803A1BC: a sprite list (8 bytes an entry, up to one whose +6 is 0xFFFF) from the
    /// next free slot, moved by (y, x).
    fn sprite_list(&mut self, list: u32, y: u8, x: u16) {
        let mut at = list;
        while self.r16(at + 6) != 0xFFFF {
            let o = self.oam_next as usize * 8;
            if o + 8 > OAM_BUF {
                break;
            }
            self.oam[o] = self.r8(at).wrapping_add(y);
            self.oam[o + 1] = self.r8(at + 1);
            let a1 = self.oam16(o + 2) & 0xFE00 | (self.r16(at + 2) & 0x1FF).wrapping_add(x) & 0x1FF;
            self.set_oam16(o + 2, a1);
            self.oam[o + 3] = self.oam[o + 3] & 1 | self.r8(at + 3) & 0xFE;
            let a2 = self.oam16(o + 4) & 0xFC00 | self.r16(at + 4) & 0x3FF;
            self.set_oam16(o + 4, a2);
            self.oam[o + 5] = self.oam[o + 5] & 3 | self.r8(at + 5) & 0xFC;
            let next = self.r16(at + 6);
            self.set_oam16(o + 6, next);
            self.oam_next += 1;
            at += 8;
        }
    }
}

// The players of the scene.
impl Title {
    /// 0x08034748: both players (Luigi first), then the props. Under the curtain they
    /// walk in place at their marks (0x08144138, facing 0x0814413A).
    fn actors(&mut self) {
        self.accel = (self.accel & 0xF0).wrapping_add(0x70);
        self.p[0].oam = 0x10;
        self.p[1].oam = 0x40;
        for i in [1usize, 0] {
            self.current = i as u8;
            if self.walk_in == 0 {
                self.run_x(i);
                self.run_y(i);
                self.animate(i);
                self.edges(i);
                if i == 0 {
                    self.puff_sprite();
                }
                if self.puff == 0 {
                    self.draw_player(i);
                }
                let p = &mut self.p[i];
                let (hi, lo) = ((p.x >> 8) as u8, p.x as u8);
                if hi != 0 {
                    if hi & 0x80 != 0 {
                        if lo < 0xC0 {
                            p.x = p.x & 0xFF00 | 0xC0;
                        }
                    } else if lo > 0x3F {
                        p.x = p.x & 0xFF00 | 0x3F;
                    }
                }
            } else {
                if self.walk_wait == 0 {
                    self.p[i].vx = 8;
                } else {
                    self.walk_wait -= 1;
                    self.p[i].vx = 0;
                }
                let mark = self.r8(0x0814_4138 + i as u32);
                let face = self.r8(0x0814_413A + i as u32);
                let p = &mut self.p[i];
                p.x = p.x & 0xFF00 | mark as u16;
                p.y = p.y & 0xFF00 | 0xA0;
                p.face = face;
                self.animate(i);
                self.draw_player(i);
                self.p[i].vx = 0;
            }
        }
        self.props_run();
    }

    /// 0x08034DBC: speed toward the held direction (0x08144174: slow down 0xFF60,
    /// speed up 0xE0, turn 0x170, in 1/256 of a speed unit, summed in +0xEE), top
    /// speed 0x18 (0x28 running); no slowing down in the air. Then 0x08034FD4 moves.
    fn run_x(&mut self, i: usize) {
        let accel = self.accel;
        let t = |k: u32| self.r16(0x0814_4174 + k * 2);
        let p = self.p[i];
        let neg = (p.vx as i8) < 0;
        let speed = if neg { p.vx.wrapping_neg() } else { p.vx };
        let dir = if neg {
            1
        } else if speed == 0 {
            0
        } else {
            2
        };
        let max = if p.held & 0x40 == 0 { 0x18u8 } else { 0x28 };
        let step = if p.held & 3 == 0 {
            if p.air != 0 || p.vx == 0 {
                None
            } else if neg {
                Some(t(0).wrapping_neg())
            } else {
                Some(t(0))
            }
        } else {
            let mut k = 2;
            let mut skip = false;
            if p.held & 3 & dir == 0 {
                k = 1;
                let d = speed.wrapping_sub(max);
                if d == 0 {
                    skip = true;
                } else if (d as i8) > 0 {
                    if p.air != 0 {
                        skip = true;
                    }
                    k = 0;
                }
            }
            if skip {
                None
            } else if p.held & 1 != 0 {
                Some(t(k))
            } else {
                Some(t(k).wrapping_neg())
            }
        };
        let p = &mut self.p[i];
        p.dir = dir;
        if let Some(s) = step {
            p.vx = p.vx.wrapping_add(((s as u32 + accel as u32) >> 8) as u8);
        }
        let f = (p.vx as u32 & 0xF) * 0x10 + p.xs as u32;
        p.xs = f as u8;
        p.x = p.x.wrapping_add((((p.vx as i8) >> 4) as i16 as u16).wrapping_add((f >> 8) as u16));
    }

    /// 0x08034ECC: a jump on pressing it on the floor (-0x38 - 0x08144180[speed >> 4]),
    /// gravity 5 (1 rising fast with it held), a raccoon's fall held to 8 with it held;
    /// 0x08035010 moves (falling at most 0x40). The floor is y 0xA0; above the screen the
    /// fall is 0x20.
    fn run_y(&mut self, i: usize) {
        let jump = self.p[i].pressed & 0x80 != 0;
        if jump && self.p[i].air == 0 {
            let speed = (self.p[i].vx as i8).unsigned_abs() as u32;
            let boost = self.r8(0x0814_4180 + ((speed >> 4) & 0xFF));
            let p = &mut self.p[i];
            p.vy = 0xC8u8.wrapping_sub(boost);
            p.air = 1;
            p.wag = 0;
        }
        let p = &mut self.p[i];
        if p.air != 0 {
            let g = if (p.vy.wrapping_add(0x20) as i8) < 0 && p.held & 0x80 != 0 { 1 } else { 5 };
            p.vy = p.vy.wrapping_add(g);
            if p.form == 2 && p.held & 0x80 != 0 && (p.vy.wrapping_sub(8) as i8) >= 0 {
                p.vy = 8;
            }
        }
        if (p.vy as i8) >= 0 && p.vy >= 0x40 {
            p.vy = 0x40;
        }
        let f = (p.vy as u32 & 0xF) * 0x10 + p.ys as u32;
        p.ys = f as u8;
        p.y = p.y.wrapping_add((((p.vy as i8) >> 4) as i16 as u16).wrapping_add((f >> 8) as u16));
        if p.y >> 8 == 0 {
            if p.y as u8 >= 0xA0 {
                p.y = 0xA0;
                p.air = 0;
                p.vy = 0;
            }
        } else {
            p.vy = 0x20;
        }
    }

    /// 0x08034858: the walk frame (faster with speed: 0x0814413C[speed >> 3]), facing, and
    /// the pose: 0x08144230[form * 4 + frame], skidding 0x13 / 0xC, ducking
    /// 0x08144240[form] (Luigi bounced), looking up 0x17, in the air (raccoon: tail wag
    /// 0x08144245), grabbing 0xD, kicking 0x16, Mario shrinking (0x0814424C[+0xD8 >> 2]),
    /// holding a shell (0x08144248).
    fn animate(&mut self, i: usize) {
        let speed = (self.p[i].vx as i8).unsigned_abs() as u32;
        let wait = self.r8(0x0814_413C + ((speed >> 3) & 0xFF));
        {
            let p = &mut self.p[i];
            p.frame_t = p.frame_t.wrapping_add(1);
            if (p.frame_t.wrapping_sub(wait) as i8) >= 0 {
                p.frame_t = 0;
                p.frame = p.frame.wrapping_add(1);
                if (p.frame.wrapping_sub(4) as i8) >= 0 {
                    p.frame = 0;
                }
            }
            if p.held & 3 == 0 && p.vx == 0 {
                p.frame = 2;
            }
        }
        let old_face = self.p[i].face;
        if self.p[i].held & 3 != 0 {
            self.p[i].face = if self.p[i].held & 2 == 0 { 0x40 } else { 0 };
        }
        let turned = self.p[i].face ^ old_face;
        let p = self.p[i];
        let mut pose = self.r8(0x0814_4230 + ((p.form as u32) << 2 | p.frame as u32));
        if p.vx.wrapping_add(1) > 2 && p.held & p.dir != 0 {
            pose = if p.form == 0 { 0x13 } else { 0x0C };
        }
        if p.held & 4 != 0 {
            pose = self.r8(0x0814_4240 + p.form as u32);
            if p.pressed & 4 != 0 {
                let o = &mut self.p[i ^ 1];
                o.vy = 0xA0;
                o.air = 0xA0;
            }
        }
        if p.held & 8 != 0 {
            pose = 0x17;
        }
        self.p[i].pose = pose;
        let p = self.p[i];
        if p.air != 0 && p.form != 0 {
            let pose = if p.form == 2 {
                if (p.vy as i8) >= 0 {
                    let wag = if p.wag == 0 {
                        if p.held & 0x80 != 0 { 10 } else { 0 }
                    } else {
                        p.wag - 1
                    };
                    self.p[i].wag = wag;
                    self.r8(0x0814_4245 + (wag as u32 >> 2))
                } else {
                    0x15
                }
            } else if (p.vy as i8) >= 0 {
                0
            } else {
                0x14
            };
            self.p[i].pose = pose;
        }
        let p = &mut self.p[i];
        if p.grab != 0 {
            p.grab -= 1;
            p.pose = 0x0D;
        }
        if p.kick != 0 {
            p.kick -= 1;
            p.pose = 0x16;
        }
        if i == 0 && self.shrink != 0 {
            let pose = self.r8(0x0814_424C + (self.shrink as u32 >> 2));
            self.p[0].pose = pose;
            self.p[0].form = if pose == 0x11 { 0 } else { 1 };
            self.shrink -= 1;
        }
        let p = self.p[i];
        if p.hold == 0 {
            if p.pressed & 0x20 == 0 {
                return;
            }
            self.p[i].hold = p.pressed & 0x20;
        }
        let frame = if p.air != 0 { 0 } else { p.frame };
        let pose = self.r8(0x0814_4248 + frame as u32);
        let p = &mut self.p[i];
        p.pose = pose;
        if turned != 0 {
            p.kick = 8;
        }
        if p.kick != 0 {
            p.kick -= 1;
            p.pose = 10;
        }
        if p.pressed & 0x40 != 0 {
            p.hold = 0;
            p.kick = 0;
            p.grab = 10;
        }
    }

    /// 0x08034C8C: which of the sprite's sides (0x0814415A offsets) are off screen.
    fn edges(&mut self, i: usize) {
        let mut edge = 0;
        let mut k: i32 = if self.p[i].face & 0x40 != 0 { 5 } else { 2 };
        loop {
            let off = self.r16(0x0814_415A + k as u32 * 2);
            if (off as u32 + self.p[i].x as u32) & 0xFFFF > 0xFF {
                edge |= self.r8(0x0814_4166 + k as u32);
            }
            k -= 1;
            if k < 0 || k == 2 {
                break;
            }
        }
        self.p[i].edge = edge;
    }

    /// 0x08034D00: Mario's puff while he turns raccoon (0x0814416C), in his slot; he stops.
    fn puff_sprite(&mut self) {
        if self.puff == 0 {
            return;
        }
        let e = self.puff;
        self.puff -= 1;
        let p = self.p[0];
        let o = (p.oam >> 2) as usize * 8;
        let a1 = self.oam16(o + 2) & 0xFE00 | p.x & 0x1FF;
        self.set_oam16(o + 2, a1);
        self.oam[o] = (p.y as u8).wrapping_sub(0x27);
        let tile = self.r16(0x0814_416C + ((e as u32 >> 2) & 3) * 2);
        let a2 = self.oam16(o + 4) & 0xFC00 | tile & 0x3FF;
        self.set_oam16(o + 4, a2);
        self.oam[o + 1] &= 0x3F;
        self.oam[o + 5] = self.oam[o + 5] & 3 | 0x24;
        self.oam[o + 3] = self.oam[o + 3] & 0xF | 0x40;
        self.p[0].vy = 0;
        self.p[0].vx = 0;
    }

    /// 0x08034B38: 0x08035AB0 points the tiles at the pose, 0x08034B54 draws the 32x32
    /// sprite (tile 0x08144154[i], palette 0x08144158[+0xDA]) 0x2F above y; small Mario
    /// grabbing goes behind the BGs.
    fn draw_player(&mut self, i: usize) {
        let p = self.p[i];
        self.p[i].tiles = Some((p.pose, p.form));
        let (x, y) = (p.x, p.y);
        if x.wrapping_add(0x20) >= 0x110 || y.wrapping_sub(0xF) >= 0xC0 {
            return;
        }
        let prio_y = if p.form == 0 && p.held & 0x10 != 0 { 0 } else { 0x20u8 };
        let x = if p.face & 0x40 != 0 { x.wrapping_sub(0x10) } else { x };
        let pal = self.r8(0x0814_4158 + self.current as u32);
        let tile = self.r8(0x0814_4154 + i as u32) as u16;
        let o = (p.oam >> 2) as usize * 8;
        let a1 = self.oam16(o + 2) & 0xFE00 | x & 0x1FF;
        self.set_oam16(o + 2, a1);
        self.oam[o] = (y as u8).wrapping_sub(0x2F);
        let a2 = self.oam16(o + 4) & 0xFC00 | tile;
        self.set_oam16(o + 4, a2);
        self.oam[o + 1] &= 0x3F;
        let hi1 = self.oam[o + 3];
        self.oam[o + 5] = self.oam[o + 5] & 3 | pal << 4 | (3u8.wrapping_sub(prio_y >> 4 & 3) & 3) << 2;
        self.oam[o + 3] = hi1 & 0x2F | 0x80 | (p.face >> 6 & 1) << 4;
    }
}

// The props, and the Koopas of the title.
impl Title {
    /// 0x08035070: props 5 to 0: state 1 starts one (0x080350C4), from 2 its routine
    /// 0x081441A4[i] runs; +0x6E counts down.
    fn props_run(&mut self) {
        for i in (0..6).rev() {
            match self.props[i].state {
                0 => {}
                1 => self.prop_start(i),
                _ => self.prop_step(i),
            }
            let t = &mut self.props[i].timer;
            *t = t.saturating_sub(1);
        }
    }

    /// 0x080350C4: at x 0x08144184[i], y 0, still; the leaf (2) floats from 4 / 0x10
    /// frames (0x08035128), prop 3 waits 0xB9 frames (0x0803516C).
    fn prop_start(&mut self, i: usize) {
        let x = self.r8(0x0814_4184 + i as u32) as u16;
        let o = &mut self.props[i];
        o.x = x;
        o.y = 0;
        o.vy = 0;
        o.vx = 0;
        o.state += 1;
        match i {
            2 => {
                o.var = 0;
                o.vx = 0;
                o.timer = 0x10;
                o.vy = 4;
            }
            3 => o.var = 0xB9,
            _ => {}
        }
    }

    fn prop_step(&mut self, i: usize) {
        match i {
            // 0x080351A4: bounces to the right on landing.
            0 => {
                if !self.fall(i) {
                    self.props[i].vx = 0x20;
                    self.props[i].vy = 0xB0;
                }
                self.prop_draw(i);
                self.prop_leaves(i);
            }
            // 0x080351EC: walks left on landing.
            1 => {
                if !self.fall(i) {
                    self.props[i].vx = 0xF0;
                }
                self.prop_draw(i);
                self.prop_leaves(i);
            }
            2 => self.leaf(i),
            3 => self.prop_shell(i),
            4 => self.prop_jumper(i),
            _ => match self.props[i].state {
                2 => self.shell_drop(i),
                3 => self.shell_kicked(i),
                4 => self.shell_luigi(i),
                5 => self.shell_mario(i),
                _ => self.shell_last(i),
            },
        }
    }

    /// 0x08035968: falls (4 a frame, at most 0x60) and moves; false once it is on the
    /// floor (y 0xB0).
    fn fall(&mut self, i: usize) -> bool {
        self.props[i].vy = self.props[i].vy.wrapping_add(4);
        self.prop_move_y(i);
        self.prop_move_x(i);
        let o = &mut self.props[i];
        if o.y as u8 & 0xF0 == 0xB0 {
            o.y = 0xB0;
            return false;
        }
        true
    }

    /// 0x08035A04.
    fn prop_move_x(&mut self, i: usize) {
        let o = &mut self.props[i];
        let f = (o.vx as u32 & 0xF) * 0x10 + o.xs as u32;
        o.xs = f as u8;
        o.x = o.x.wrapping_add((((o.vx as i8) >> 4) as i16 as u16).wrapping_add((f >> 8) as u16));
    }

    /// 0x08035A48.
    fn prop_move_y(&mut self, i: usize) {
        let o = &mut self.props[i];
        if (o.vy as i8) >= 0 && o.vy >= 0x60 {
            o.vy = 0x60;
        }
        let f = (o.vy as u32 & 0xF) * 0x10 + o.ys as u32;
        o.ys = f as u8;
        o.y = o.y.wrapping_add((((o.vy as i8) >> 4) as i16 as u16).wrapping_add((f >> 8) as u16));
    }

    /// 0x080359B8: gone once off screen.
    fn prop_leaves(&mut self, i: usize) {
        let o = &mut self.props[i];
        let hi = (o.x >> 8) as u8;
        if hi != 0 {
            let edge = self.rom.rom_u16(0x0814_4220 + (hi as u32 >> 7) * 2);
            if (edge as u32 + o.x as u32) & 0xFFFF > 0xFF {
                o.state = 0;
            }
        }
    }

    /// 0x08035760: walking props step their frame and flip with the frame counter.
    fn prop_walk(&mut self, i: usize) {
        if self.props[i].vx != 0 && self.shrink == 0 {
            let f = self.frame;
            self.props[i].frame = (f & 6) >> 1;
            self.props[i].flip = (f & 4) << 4;
        }
        self.prop_draw(i);
    }

    /// 0x08035840: in its own slot 0x08144214[i].
    fn prop_draw(&mut self, i: usize) {
        let slot = self.r8(0x0814_4214 + i as u32);
        self.prop_draw_at(i, slot);
    }

    /// 0x08035858: a 16x16 sprite 0x2F above y, tile 0x081441FC[i][frame], palette
    /// 0x0814421A[i], from OAM slot 0x40.
    fn prop_draw_at(&mut self, i: usize, slot: u8) {
        let o = self.props[i];
        if o.x.wrapping_add(0x20) >= 0x110 || o.y.wrapping_sub(0xF) >= 0xC0 {
            return;
        }
        let e = (0x40 + (slot >> 2) as usize) * 8;
        let tiles = self.r32(0x0814_41FC + i as u32 * 4);
        let tile = self.r16(tiles + o.frame as u32 * 2);
        let pal = self.r8(0x0814_421A + i as u32);
        let a1 = self.oam16(e + 2) & 0xFE00 | o.x & 0x1FF;
        self.set_oam16(e + 2, a1);
        self.oam[e] = (o.y as u8).wrapping_sub(0x2F);
        let a2 = self.oam16(e + 4) & 0xFC00 | tile & 0x3FF;
        self.set_oam16(e + 4, a2);
        self.oam[e + 1] &= 0x3F;
        self.oam[e + 5] = self.oam[e + 5] & 3 | pal << 4 | 4;
        self.oam[e + 3] = self.oam[e + 3] & 0xF | 0x40 | (o.flip >> 6 & 1) << 4 | (o.flip >> 7) << 5;
    }

    /// 0x080357C8: carried in front of player `by` (0x081441D8[kick, + 8 facing right]),
    /// 0xD below; drawn first while kicked.
    fn prop_carried(&mut self, i: usize, by: usize) {
        let p = self.p[by];
        let k = if p.face != 0 { p.kick.wrapping_add(8) } else { p.kick };
        let dx = self.r8(0x0814_41D8 + k as u32) as i8;
        let o = &mut self.props[i];
        o.x = p.x.wrapping_add(dx as i16 as u16);
        o.y = p.y.wrapping_add(0xD);
        if p.kick == 0 {
            self.prop_draw(i);
        } else {
            self.prop_draw_at(i, 0);
        }
    }

    /// 0x08035228: the leaf sways down (0x081441BC-C0); at y 0x50 Mario turns raccoon.
    fn leaf(&mut self, i: usize) {
        let mut sway = true;
        if self.props[i].timer != 0 {
            self.prop_move_y(i);
            let o = &mut self.props[i];
            o.vy = o.vy.wrapping_add(1);
            if o.vy != 0 {
                sway = false;
            } else {
                o.timer = 0;
            }
        }
        if sway {
            let u = self.props[i].var as u32 & 1;
            let (dv, end) = (self.r8(0x0814_41BC + u), self.r8(0x0814_41BE + u));
            let o = &mut self.props[i];
            o.vx = o.vx.wrapping_add(dv);
            if o.vx == end {
                o.var = o.var.wrapping_add(1);
            }
            let u = if (o.vx as i8) < 0 { u + 1 } else { u };
            o.vy = self.rom.rom_u8(0x0814_41C0 + u);
            self.prop_move_x(i);
            self.prop_move_y(i);
        }
        let o = &mut self.props[i];
        if o.vx != 0 {
            o.flip = if (o.vx as i8) < 0 { 0x40 } else { 0 };
        }
        self.prop_draw(i);
        let o = &mut self.props[i];
        if o.y as u8 & 0xF0 == 0x50 {
            o.state = 0;
            self.puff = 0x13;
            self.p[0].form = 2;
        }
    }

    /// 0x08035354: lands, slides in from the right to x 0x30, then stays a while; Mario
    /// hops off it.
    fn prop_shell(&mut self, i: usize) {
        if self.fall(i) {
            self.props[i].flip = (self.frame & 8) << 3;
            self.prop_draw(i);
            return;
        }
        let o = self.props[i];
        if o.var != 0 {
            self.props[i].var -= 1;
        } else {
            if o.x as u8 > 0x2F {
                self.props[i].vx = 0xF8;
                self.props[i].flip = (self.frame & 8) << 3;
                self.prop_draw(i);
                return;
            }
            let o = &mut self.props[i];
            if o.timer == 0 {
                o.timer = 0x10;
            }
            if o.timer == 1 {
                o.state = 0;
            }
            if o.vx != 0 {
                o.vx = 0;
                self.p[0].vy = 0xD0;
            }
            self.props[i].frame = 1;
        }
        self.prop_draw(i);
    }

    /// 0x08035418: waits on the floor until prop 5 comes within 0x10, then flies up and
    /// off upside down.
    fn prop_jumper(&mut self, i: usize) {
        if self.props[i].var == 0 {
            let near = (self.props[i].x as u8).wrapping_sub(self.props[5].x as u8) <= 0xF;
            if !near {
                if !self.fall(i) {
                    let o = &mut self.props[i];
                    o.vy = if o.vy < 0x20 { 0 } else { 0x20u8.wrapping_sub(o.vy) };
                }
                self.prop_draw(i);
                return;
            }
            self.props[i].var = 1;
        }
        let o = &mut self.props[i];
        if o.vx == 0 {
            o.vx = 0x10;
            o.vy = 0xC0;
            o.flip = 0x80;
        }
        o.vy = o.vy.wrapping_add(4);
        self.prop_move_x(i);
        self.prop_move_y(i);
        if self.props[i].y >> 8 == 0 {
            self.prop_draw(i);
        } else {
            self.props[i].state = 0;
        }
    }

    /// Prop 5's states (0x081441C4): 0x08035510 drops until Mario kicks it off at y 0x90.
    fn shell_drop(&mut self, i: usize) {
        if !self.fall(i) {
            self.props[i].state += 1;
            self.props[i].vx = 0;
        } else {
            let o = &mut self.props[i];
            if o.y as u8 > 0x8F && o.vx == 0 {
                o.vx = 0xE8;
                o.vy = 0xE8;
                self.p[0].kick = 0x10;
            }
        }
        self.prop_walk(i);
    }

    /// 0x0803558C: on the floor; Mario's grab (button 0x10) sends it right; Luigi picks it
    /// up.
    fn shell_kicked(&mut self, i: usize) {
        self.fall(i);
        if self.p[0].pressed & 0x10 != 0 {
            self.p[0].grab = 10;
            self.props[i].vx = 0x30;
        }
        if self.props[i].x >> 8 != 0 {
            self.props[i].vx = 0;
        }
        if self.p[1].hold != 0 {
            self.props[i].state += 1;
        }
        self.prop_walk(i);
    }

    /// 0x080355FC: carried by Luigi, then thrown left; it stops at x 0x38 and Mario hops.
    fn shell_luigi(&mut self, i: usize) {
        if self.p[1].hold != 0 {
            self.props[i].vx = 0xD0;
            self.prop_carried(i, 1);
            return;
        }
        self.fall(i);
        let o = &mut self.props[i];
        if (o.x as u8) < 0x38 && o.vx != 0 {
            o.vx = 0;
            self.p[0].vy = 0xD0;
        }
        if self.p[0].hold != 0 {
            self.props[i].state += 1;
        }
        self.prop_walk(i);
    }

    /// 0x08035680: carried by Mario, then thrown right off screen; back from the left
    /// 0x30 frames later.
    fn shell_mario(&mut self, i: usize) {
        if self.p[0].hold != 0 {
            self.prop_carried(i, 0);
            return;
        }
        self.fall(i);
        let o = &mut self.props[i];
        o.vx = 0x30;
        if o.x >> 8 == 0 {
            self.prop_walk(i);
        } else {
            o.x = 0xFFF0;
            o.timer = 0x30;
            o.state += 1;
        }
    }

    /// 0x080356F0: slides in and hits big Mario (+0xD8 = 0x2C: he shrinks).
    fn shell_last(&mut self, i: usize) {
        if self.props[i].timer != 0 {
            return;
        }
        if self.shrink == 0 {
            self.fall(i);
            let o = self.props[i];
            if o.x >> 8 == 0 && self.p[0].form != 0 && (self.p[0].x as u8).wrapping_sub(o.x as u8) < 0x10 {
                self.shrink = 0x2C;
            }
        }
        self.prop_leaves(i);
        self.prop_walk(i);
    }

    /// 0x08035C64: Koopas 3 to 0 walk in from the left (0x08035CA8: x -0x10, speed 8,
    /// 0x12 for the last); past 0x08144228[i] the next starts (0x08035CF4). 16x32 sprites
    /// at y 0x73, slots 0x10 + 0x08144224[i] / 4, frames 0x0814422C by frame counter
    /// bit 3 (2 for the last).
    fn koopas(&mut self) {
        for i in (0..4).rev() {
            match self.props[i].state {
                0 => {}
                1 => {
                    let o = &mut self.props[i];
                    o.state += 1;
                    o.x = 0xFFF0;
                    if i == 3 {
                        o.vx = 0x12;
                    } else {
                        o.vx = 8;
                    }
                }
                _ => self.koopa(i),
            }
        }
    }

    fn koopa(&mut self, i: usize) {
        self.prop_move_x(i);
        self.prop_leaves(i);
        let gap = self.r8(0x0814_4228 + i as u32);
        let o = self.props[i];
        if o.x >> 8 == 0 && gap != 0 && gap < o.x as u8 {
            let next = &mut self.props[i + 1];
            if next.state == 0 {
                next.state = 1;
            }
        }
        let f = if i != 3 { self.frame >> 3 } else { self.frame >> 2 };
        self.props[i].frame = f & 1;
        let x = self.props[i].x;
        if x.wrapping_add(0x20) >= 0x110 {
            return;
        }
        let e = (0x10 + (self.r8(0x0814_4224 + i as u32) >> 2) as usize) * 8;
        let tile = self.r16(0x0814_422C + self.props[i].frame as u32 * 2);
        let a1 = self.oam16(e + 2) & 0xFE00 | x & 0x1FF;
        self.set_oam16(e + 2, a1);
        self.oam[e] = 0x73;
        let a2 = self.oam16(e + 4) & 0xFC00 | tile & 0x3FF;
        self.set_oam16(e + 4, a2);
        self.oam[e + 1] = self.oam[e + 1] & 0x3F | 0x80;
        self.oam[e + 5] = self.oam[e + 5] & 3 | 0x28;
        self.oam[e + 3] = self.oam[e + 3] & 0x1F | 0x90;
    }
}

// The menus.
impl Title {
    /// Phase 7, 0x0802DCB4: the file select (0x08140C28[+0xF1]).
    fn files_screen(&mut self, keys: u16) {
        self.koopas();
        match self.menu {
            0 => self.files_setup(),
            1 => self.files_input(keys),
            _ => self.erase_screen(keys),
        }
        if self.phase == 7 {
            self.file_icons();
        }
    }

    /// 0x0802D914 (0x0802DC1C sparkles only files 0x10-0x17): each file's icon, two
    /// affine double-size 32x16 halves (tiles 0x20B / 0x20C) from slot 0x7A, matrix 1:1
    /// (0x08049C88). A new file's are not placed: they stay hidden at y 0xA0.
    fn file_icons(&mut self) {
        let mut o = 0x7A * 8;
        let mut affine = 0u8;
        for f in 0..3 {
            if self.files[f] & 0x80 != 0 {
                continue;
            }
            let (hi1, hi1b) = (self.oam[o + 3], self.oam[o + 11]);
            let a2 = self.oam16(o + 4) & 0xFC00 | 0x20B;
            self.set_oam16(o + 4, a2);
            let a2 = self.oam16(o + 12) & 0xFC00 | 0x20C;
            self.set_oam16(o + 12, a2);
            self.oam[o + 5] &= 0xF;
            self.oam[o + 13] &= 0xF;
            self.oam[o + 1] = self.oam[o + 1] & 0x3F | 0x43;
            self.oam[o + 9] = self.oam[o + 9] & 0x3F | 0x43;
            let group = (affine >> 2 & 7) << 1;
            self.oam[o + 3] = hi1 & 0x31 | 0x80 | group;
            self.oam[o + 11] = hi1b & 0x31 | 0x80 | group;
            // BIOS ObjAffineSet, scale 0x100, angle 0: pa = pd = 0x100, pb = pc = 0.
            for (k, v) in [0x100u16, 0, 0, 0x100].into_iter().enumerate() {
                let at = (affine as usize + k) * 8 + 6;
                self.set_oam16(at, v);
            }
            affine += 4;
            o += 0x10;
        }
    }

    /// 0x0802C9D0: BG1 shows the file select (its right half, x 0x100), the files' rows
    /// (0x0802B5C4) and the line under them (0x0802C88C).
    fn files_setup(&mut self) {
        self.bg1.0 = 0x100;
        self.load_map(MAP_FILES, 1);
        // 0x0802B5C4: per file, 9 entries of 0x08140124[state & 0x1F] from map + 0x99A
        // (rows 0x80 bytes apart), and the cursor's row lit (0x0802B54C, palette 8).
        self.row_colour(self.cursor, 8);
        let map = self.bg1_map();
        for f in 0..3 {
            let list = self.r32(0x0814_0124 + (self.files[f] as u32 & 0x1F) * 4);
            for k in 0..9 {
                let v = self.r16(list + k * 2);
                self.map_write(map + 0x99A + f * 0x80 + k as usize * 2, v);
            }
        }
        // 0x0802C88C: no e-Reader cards (0x03003A4D = 0xFF): 0x081408D0[lang] entries of
        // 0x081408BC[lang] at map + 0x324.
        let n = self.r8(0x0814_08D0 + LANGUAGE);
        let list = self.r32(0x0814_08BC + LANGUAGE * 4);
        for k in 0..n as u32 {
            let v = self.r16(list + k * 2);
            self.map_write(map + 0x324 + k as usize * 2, v);
        }
        self.menu += 1;
    }

    /// 0x0802B54C -> 0x0802B000 (English): row `r` of the file select in palette `pal`
    /// (8 lit, 9 dim): entries 0x0813FBC0[r] at map + 0x0813FBF0[r], two lines of
    /// 0x0813FC00[r * 2], [r * 2 + 1] entries. The erase prompt's row 4 is row 5.
    fn row_colour(&mut self, r: u8, pal: u16) {
        let r = if self.menu == 2 && self.erase == 2 && r == 4 { 5 } else { r as u32 };
        let map = self.bg1_map() + self.r16(0x0813_FBF0 + r * 2) as usize;
        let mut list = self.r32(0x0813_FBC0 + r * 4);
        for line in 0..2 {
            let n = self.r8(0x0813_FC00 + r * 2 + line);
            for k in 0..n as usize {
                let v = pal << 12 | self.r16(list);
                self.map_write(map + line as usize * 0x40 + k * 2, v);
                list += 2;
            }
        }
    }

    /// 0x0802CB90: Up / Down between the files and Erase File (skipping the hidden e-Reader
    /// row 3), A / Start opens the file (new: the mode select) or the erase prompt, B back
    /// to the title. The hand points at the row (0x0802C9F4).
    fn files_input(&mut self, keys: u16) {
        let mut u = self.cursor;
        if keys & KEY_B != 0 {
            self.phase = 15;
        } else if keys & (KEY_START | KEY_A) != 0 {
            if u < 3 {
                if self.files[u as usize] & 0x80 == 0 {
                    self.phase = 8;
                    self.menu = 0;
                }
            } else if u == 4 {
                // The erase prompt: its text (0x0802CA80), BG3 and the backdrop darkened.
                self.menu = 2;
                self.erase = 0;
                self.erase_text();
                self.bldy = 2;
                self.bldcnt = BLD_ERASE;
            }
            return;
        } else if keys & KEY_UP != 0 {
            u = if u == 4 { 2 } else { u.saturating_sub(1) };
        } else if keys & KEY_DOWN != 0 {
            if u == 2 {
                u = 4;
            } else if u != 4 {
                u += 1;
            }
        }
        if u != self.cursor {
            self.row_colour(self.cursor, 9);
            self.row_colour(u, 8);
            self.cursor = u;
        }
        self.hand(u);
    }

    /// The hand (sprite list 0x0813FA20) at the file row (0x08140937) and the language's
    /// x (0x0814093C).
    fn hand(&mut self, row: u8) {
        let y = self.r8(0x0814_0937 + row as u32);
        let x = self.r8(0x0814_093C + LANGUAGE) as u16;
        self.sprite_list(0x0813_FA20, y, x);
    }

    /// 0x0802CA80: "Select a file to erase." over the menu's top line (0x08140944[lang],
    /// 0x08140962 entries a line, then 0x0814096C blanks), from map + 0xB00 +
    /// 0x08140958[lang] * 2.
    fn erase_text(&mut self) {
        let lang = LANGUAGE;
        let map = self.bg1_map() + 0xB00 + self.r16(0x0814_0958 + lang * 2) as usize * 2;
        let mut list = self.r32(0x0814_0944 + lang * 4);
        let n = self.r8(0x0814_0962 + lang * 2);
        let second = self.r16(0x0814_0962 + lang * 2) as u32;
        let mut at = map;
        for _ in 0..n {
            let (a, b) = (self.r16(list), self.r16(list + second * 2));
            self.map_write(at, a);
            self.map_write(at + 0x40, b);
            at += 2;
            list += 2;
        }
        for _ in 0..self.r8(0x0814_096C + lang * 2) {
            self.map_write(at, 0x5C);
            self.map_write(at + 0x40, 0x5C);
            at += 2;
        }
    }

    /// The erase prompt (0x08140BE8[+0xF5]): its sign slides in (0x0802CEE8, 4 frames from
    /// 0x08140988 shown 0x08140998 frames each), then a file is chosen (0x0802D1D4: a new
    /// file just buzzes; Erase File again or B close it), then the sign slides out and the
    /// file select returns (0x0802D8CC).
    fn erase_screen(&mut self, keys: u16) {
        match self.erase {
            0 => {
                // 0x0802D0D8
                self.msg = 0;
                self.msg_t = 3;
                self.erase += 1;
            }
            1 => {
                if !self.erase_sign() {
                    self.erase += 1;
                }
            }
            2 => self.erase_choose(keys),
            _ => {
                if !self.erase_sign() {
                    self.menu = 1;
                }
            }
        }
    }

    fn erase_sign(&mut self) -> bool {
        let list = self.r32(0x0814_0988 + self.msg as u32 * 4);
        let y = self.r8(0x0814_0937 + self.cursor as u32);
        let x = self.r8(0x0814_093C + LANGUAGE) as u16;
        self.sprite_list(list, y, x);
        self.msg_t = self.msg_t.wrapping_sub(1);
        if self.msg_t == 0 {
            self.msg += 1;
            if self.msg == 4 {
                return false;
            }
            self.msg_t = self.r8(0x0814_0998 + self.msg as u32);
        }
        true
    }

    /// 0x0802D1D4.
    fn erase_choose(&mut self, keys: u16) {
        let mut u = self.cursor;
        let close = |t: &mut Title, u: u8| {
            t.erase = 5;
            t.msg_t = 8;
            t.msg = 0;
            t.row_colour(4, if u == 4 { 8 } else { 9 });
            // 0x0802D1A8: the darkening off.
            t.bldy = 0;
            t.bldcnt = BLD_ALL;
        };
        if keys & KEY_B != 0 {
            close(self, u);
            return;
        }
        if keys & (KEY_START | KEY_A) != 0 {
            if u == 4 {
                close(self, u);
            }
            // A new file: the buzzer (sound 0x1A), nothing to erase.
            return;
        }
        if keys & KEY_UP != 0 {
            u = if u == 4 { 2 } else { u.saturating_sub(1) };
        } else if keys & KEY_DOWN != 0 {
            if u == 2 {
                u = 4;
            } else if u != 4 {
                u += 1;
            }
        }
        if u != self.cursor {
            self.row_colour(self.cursor, 9);
            self.row_colour(u, 8);
            self.cursor = u;
            self.msg_t = 0;
        }
        // 0x0802CD34: the eraser hand (0x0813F9F0) blinks (+0xFA bit 3).
        if self.msg_t & 8 == 0 {
            let y = self.r8(0x0814_0937 + u as u32);
            let x = self.r8(0x0814_093C + LANGUAGE) as u16;
            self.sprite_list(0x0813_F9F0, y, x);
        }
        self.msg_t = self.msg_t.wrapping_add(1);
    }

    /// 0x0802DD50: BG1 shows the mode select (left half), MARIO lit (0x0802DCF4).
    fn modes_setup(&mut self) {
        self.menu = 1;
        self.load_map(MAP_MODES, 1);
        self.bg1.0 = 0;
        self.mode = 0;
        self.mode_colour(self.mode, 8);
    }

    /// 0x0802DCF4: mode row `r` (12 entries of 0x08140C64[r] at map + 0x192 + r * 0x80) in
    /// palette `pal`.
    fn mode_colour(&mut self, r: u8, pal: u16) {
        let map = self.bg1_map() + 0x192 + r as usize * 0x80;
        let list = self.r32(0x0814_0C64 + r as u32 * 4);
        for k in 0..12 {
            let v = pal << 12 | self.r16(list + k * 2);
            self.map_write(map + k as usize * 2, v);
        }
    }

    /// 0x0802DDEC: Up / Down between MARIO and MARIO & LUIGI, A / Start starts, B back
    /// to the title; the hand at y 0x2A + 0x10 * mode, x 0x36 (0x0802DDCC).
    fn modes_input(&mut self, keys: u16) {
        if keys & KEY_B != 0 {
            self.phase = 15;
        } else if keys & (KEY_START | KEY_A) != 0 {
            self.phase = 4;
        } else {
            let m = self.mode;
            let next = if (keys & KEY_UP != 0 && m != 0) || (keys & KEY_DOWN != 0 && m == 0) { m ^ 1 } else { m };
            if next != m {
                self.mode = next;
                self.mode_colour(next, 8);
                self.mode_colour(next ^ 1, 9);
            }
        }
        self.sprite_list(0x0813_FA20, 0x2A + self.mode * 0x10, 0x36);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bare() -> Title {
        Title::new(Arc::new(TitleAsset::default()))
    }

    #[test]
    fn setup_waits_then_fades_in_and_the_curtain_rises() {
        let mut t = bare();
        let mut frames = 0;
        while t.step < 4 {
            t.step(Buttons::empty());
            frames += 1;
        }
        // 9 + 6 setup frames, 0x10 of fade-in, 0x36 waiting.
        assert_eq!(frames, 9 + 6 + 0x10 + 0x36);
        assert_eq!((t.bldy, t.bldcnt), (0, 0));
        let mut rise = 0;
        while t.phase == 0 {
            t.step(Buttons::empty());
            rise += 1;
        }
        assert_eq!(rise, 0xA8 / 2, "2 px a frame up to 0xA8");
    }

    #[test]
    fn start_skips_to_the_title_and_a_opens_the_file_select() {
        let mut t = bare();
        while t.step < 3 {
            t.step(Buttons::empty());
        }
        t.step(Buttons::START);
        while t.step < 4 {
            t.step(Buttons::empty());
        }
        t.step(Buttons::empty());
        assert_eq!(t.phase, 2, "skipped (phase 6 -> 2)");
        t.step(Buttons::empty());
        assert_eq!(t.phase, 3);
        t.step(Buttons::A);
        assert_eq!(t.phase, 7);
        t.step(Buttons::empty());
        t.step(Buttons::DOWN);
        assert_eq!(t.cursor, 1);
        t.step(Buttons::UP);
        t.step(Buttons::A);
        assert_eq!(t.phase, 8, "a new file: the mode select");
        t.step(Buttons::empty());
        t.step(Buttons::START);
        assert_eq!(t.phase, 4);
        let mut ev = None;
        for _ in 0..40 {
            ev = ev.or(t.step(Buttons::empty()));
        }
        assert_eq!(ev, Some(TitleEvent::Start { players: 1 }));
    }

    #[test]
    fn the_title_times_out_into_the_scene_again() {
        let mut t = bare();
        t.step = 4;
        t.phase = 3;
        t.timeout = (1, 1);
        t.step(Buttons::empty());
        assert!(t.fade.is_some());
        let mut n = 0;
        while t.fade.is_some() {
            t.step(Buttons::empty());
            n += 1;
        }
        assert_eq!(n, 15, "8 palette steps, every other frame");
        assert_eq!(t.step, 0);
    }
}

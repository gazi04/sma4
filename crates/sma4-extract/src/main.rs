//! Extracts game assets from the user's own SMA4 ROM into a local directory
//! (default `assets/extracted/`). The game then runs from those files alone.
//!
//! The output is derived from the ROM: keep it on your machine, never commit or share it.

mod castle;
mod emu;
mod ending;
mod level;
mod oam;
mod objects;
mod screens;
mod sound;
mod spade;
mod title;
mod worldmap;

use std::path::PathBuf;

use sma4_assets::{DEFAULT_DIR, Rgba, gfx_path, write_manifest};
use sma4_rom::{Rom, gfx};

const USAGE: &str =
    "usage: sma4-extract <rom.gba> [--out <dir>] [--level <id>:<tileset>:<enemies> ...]
  --out    output directory (default assets/extracted)
  --level  level to extract: hex level id, tileset, hex enemy-list id; repeatable
           (default: every course level of World 1's map, 1-1 first)
           areas behind pipes (each level's alternate area) are extracted as well";

const SHEET_COLS: usize = 32;

struct Args {
    rom: String,
    out: PathBuf,
    levels: Vec<(u16, u8, u16)>,
}

fn parse_args() -> Option<Args> {
    let mut rom = None;
    let mut out = PathBuf::from(DEFAULT_DIR);
    let mut levels = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => out = it.next()?.into(),
            "--level" => {
                let spec = it.next()?;
                let mut parts = spec.split(':');
                let hex = |s: &str| u16::from_str_radix(s.trim_start_matches("0x"), 16).ok();
                let id = hex(parts.next()?)?;
                let tileset = parts.next()?.parse().ok()?;
                let enemies = hex(parts.next()?)?;
                levels.push((id, tileset, enemies));
            }
            "-h" | "--help" => return None,
            _ => rom = Some(a),
        }
    }
    Some(Args {
        rom: rom?,
        out,
        levels,
    })
}

/// Every LZ77 tile blob as a greyscale 4bpp sheet, for the in-game graphics viewer.
fn extract_gfx(rom: &Rom, args: &Args) -> Result<usize, String> {
    let grey: Vec<gfx::Bgr555> = (0..16u16)
        .map(|i| gfx::Bgr555((i * 31 / 15) * 0x421))
        .collect();
    let blobs = rom.scan_lz77(256, gfx::TILE_BYTES_4BPP);
    for b in &blobs {
        let data = rom.decompress(b.addr).map_err(|e| e.to_string())?;
        let (pixels, w, h) = gfx::tile_sheet_rgba(&gfx::tiles_4bpp(&data), &grey, SHEET_COLS);
        Rgba {
            width: w as u32,
            height: h as u32,
            pixels,
        }
        .write_png(&gfx_path(&args.out, b.addr))
        .map_err(|e| e.to_string())?;
    }
    Ok(blobs.len())
}

/// World 1-3 (level 0x0C), whose object graphics include the Boomerang Bro.
const BOOMERANG_BRO_LEVEL: u16 = 0x0C;
/// World 1's first Hammer Bro arena (level 0x4E), whose object graphics include the Hammer
/// Bro and the chest.
const HAMMER_BRO_LEVEL: u16 = 0x4E;
/// World 1-5 (level 0x82), with the Buzzy Beetles.
const BUZZY_LEVEL: u16 = 0x82;
/// World 1-4 (level 0xD2), with the falling lifts and the red Paratroopa.
const LIFT_LEVEL: u16 = 0xD2;
/// World 1's mini-fortress (level 0x1CB) and its other area (0x1CC).
const FORTRESS_LEVEL: u16 = 0x1CB;
const FORTRESS_ROOMS: u16 = 0x1CC;
/// World 1-3's hidden Toad House: level 0x106, tileset 7, enemy list 0x0816BA52.
const TOAD_HOUSE_1_3: u16 = 0x106;
const TOAD_HOUSE_TILESET: u8 = 7;
const TOAD_HOUSE_1_3_LIST: u32 = 0x0816_BA52;
const MAP_HOUSE: u16 = 0x102;
/// World index of the Warp Zone (World 9).
const WARP_ZONE: u8 = 8;
const MAP_HOUSE_LIST: u32 = 0x0816_BA4C;
/// The King's castle (World 1's map tile 0xC9): the throne room.
const THRONE_ROOM: u16 = 0x1C5;
/// Its junction chain: the airship's intro, the airship, Larry's cabin.
const AIRSHIP_INTRO: u16 = 0x202;
const AIRSHIP: u16 = 0x205;
const AIRSHIP_CABIN: u16 = 0x213;

fn run(args: &Args) -> Result<(), String> {
    let rom = Rom::open(&args.rom).map_err(|e| format!("{}: {e}", args.rom))?;
    let h = rom.header();
    if !matches!(h.version, sma4_rom::Version::Europe { revision: 0 }) {
        return Err(format!(
            "{:?} is not supported yet: ROM addresses are only mapped for Europe (AX4P) rev 0",
            h.version
        ));
    }
    println!("ROM: {} {} {:?}", h.title, h.game_code, h.version);
    println!("output: {}", args.out.display());

    let mut m = emu::Machine::new(rom.bytes()).map_err(|e| e.to_string())?;
    sound::extract_sounds(&rom, &args.out).map_err(|e| e.0)?;
    // By default: every course level World 1's map can enter (1-1 first).
    let levels = if args.levels.is_empty() {
        worldmap::course_levels(&rom, &mut m, 0).map_err(|e| e.0)?
    } else {
        args.levels.clone()
    };
    let mut levels = levels;
    // The battle arenas of the map's path junctions (Hammer Bros).
    if args.levels.is_empty() {
        for l in worldmap::battle_levels(&rom, &mut m, 0).map_err(|e| e.0)? {
            if !levels.iter().any(|k| k.0 == l.0) {
                levels.push(l);
            }
        }
    }
    let course_levels = levels.len();
    // Where end-of-level pipes lead (0x080056A0), and World 1's end room with the areas.
    let rooms = level::end_rooms(&rom).map_err(|e| e.0)?;
    sma4_assets::write_end_rooms(&args.out, &rooms).map_err(|e| e.to_string())?;
    let r = rooms[0];
    if !levels.iter().any(|l| l.0 == r.level) {
        levels.push((r.level, r.tileset, r.enemies));
    }
    let mut n = 0;
    let mut letter = None;
    while n < levels.len() {
        let (id, tileset, enemies) = levels[n];
        let asset = match level::extract(&rom, &mut m, id, tileset, enemies) {
            Ok(a) => a,
            // A pipe's area that does not load is skipped (its pipe then stays shut).
            Err(e) if n >= course_levels => {
                println!(
                    "  level {id:#x} ({:#010x}): {}, skipped",
                    level::level_addr(&rom, id),
                    e.0
                );
                levels.remove(n);
                continue;
            }
            Err(e) => {
                return Err(format!(
                    "level {id:#x} ({:#010x}): {}",
                    level::level_addr(&rom, id),
                    e.0
                ));
            }
        };
        asset.write(&args.out).map_err(|e| e.to_string())?;
        // Areas behind pipes (the header's alternate level) are extracted too.
        if let Some(j) = &asset.junction
            && !levels.iter().any(|l| l.0 == j.level)
        {
            levels.push((j.level, j.tileset, j.enemies));
        }
        // World 1-3 loads the Boomerang Bro's graphics.
        // World 1-5 loads the Buzzy Beetle's graphics.
        if id == BUZZY_LEVEL {
            objects::extract_koopa_kind(&rom, &mut m, &args.out, 0x70).map_err(|e| e.0)?;
            objects::extract_ceiling_piranha(&rom, &mut m, &args.out).map_err(|e| e.0)?;
            objects::extract_water(&rom, &m, &args.out).map_err(|e| e.0)?;
        }
        // The mini-fortress and its secret room.
        if id == FORTRESS_LEVEL {
            objects::extract_fortress(&rom, &mut m, &args.out).map_err(|e| e.0)?;
        }
        if id == FORTRESS_ROOMS {
            objects::extract_fortress_chest(&rom, &mut m, &args.out).map_err(|e| e.0)?;
        }
        // World 1-4 loads the lift's graphics.
        if id == LIFT_LEVEL {
            objects::extract_lift(&rom, &mut m, &args.out).map_err(|e| e.0)?;
            objects::extract_advance_coin(&rom, &mut m, &args.out).map_err(|e| e.0)?;
        }
        // The King's castle: the throne room (its junction chain leads on to the airship).
        if id == THRONE_ROOM {
            castle::extract_throne(&rom, &mut m, &args.out, id).map_err(|e| e.0)?;
            letter = Some(ending::extract_letter(&rom, &mut m, &args.out).map_err(|e| e.0)?);
        }
        if id == AIRSHIP_INTRO {
            castle::extract_intro(&mut m, &args.out, id).map_err(|e| e.0)?;
        }
        if id == AIRSHIP {
            castle::extract_sky(&rom, &mut m, &args.out).map_err(|e| e.0)?;
        }
        if id == AIRSHIP {
            castle::write_bank(&m, &args.out, id).map_err(|e| e.0)?;
        }
        if id == AIRSHIP_CABIN {
            castle::extract_cabin(&rom, &mut m, &args.out, id).map_err(|e| e.0)?;
        }
        if id == HAMMER_BRO_LEVEL {
            objects::extract_hammer_bro(&rom, &mut m, &args.out).map_err(|e| e.0)?;
        }
        if id == BOOMERANG_BRO_LEVEL {
            objects::extract_boomerang_bro(&rom, &mut m, &args.out).map_err(|e| e.0)?;
            // Its hidden Toad House (trigger 0x07, 0x0804E718), with the list the trigger
            // names directly.
            level::extract_at(&rom, &mut m, TOAD_HOUSE_1_3, TOAD_HOUSE_TILESET, TOAD_HOUSE_1_3_LIST)
                .and_then(|a| a.write(&args.out).map_err(|e| level::ExtractError(e.to_string())))
                .map_err(|e| e.0)?;
            objects::extract_toad_house(&rom, &mut m, &args.out).map_err(|e| e.0)?;
            // The map's Toad Houses (level 0x102, list 0x0816BA4C: Toad of kind 1).
            level::extract_at(&rom, &mut m, MAP_HOUSE, TOAD_HOUSE_TILESET, MAP_HOUSE_LIST)
                .and_then(|a| a.write(&args.out).map_err(|e| level::ExtractError(e.to_string())))
                .map_err(|e| e.0)?;
            objects::extract_map_house(&rom, &mut m, &args.out).map_err(|e| e.0)?;
        }
        // Object graphics are loaded by the same routines; take them after the first level.
        if n == 0 {
            objects::extract_coin(&rom, &m, &args.out).map_err(|e| e.0)?;
            objects::extract_walker(&rom, &m, &args.out).map_err(|e| e.0)?;
            objects::extract_sparkle(&m, &args.out).map_err(|e| e.0)?;
            objects::extract_brick_debris(&m, &args.out).map_err(|e| e.0)?;
            objects::extract_skid_dust(&rom, &m, &args.out).map_err(|e| e.0)?;
            objects::extract_hud(&rom, &m, &args.out).map_err(|e| e.0)?;
            objects::extract_autoscroll(&rom, &args.out).map_err(|e| e.0)?;
            objects::extract_score_popups(&rom, &m, &args.out).map_err(|e| e.0)?;
            objects::extract_koopas(&rom, &mut m, &args.out).map_err(|e| e.0)?;
            objects::extract_paragoomba(&rom, &mut m, &args.out).map_err(|e| e.0)?;
            objects::extract_micro_goomba(&rom, &mut m, &args.out).map_err(|e| e.0)?;
            objects::extract_piranhas(&rom, &mut m, &args.out).map_err(|e| e.0)?;
            objects::extract_puff(&rom, &mut m, &args.out).map_err(|e| e.0)?;
            objects::extract_goal(&rom, &mut m, &args.out).map_err(|e| e.0)?;
            objects::extract_course_clear(&rom, &mut m, &args.out).map_err(|e| e.0)?;
            objects::extract_growth_item(&rom, &mut m, &args.out).map_err(|e| e.0)?;
            objects::extract_leaf(&rom, &mut m, &args.out).map_err(|e| e.0)?;
            objects::extract_flower_and_star(&rom, &mut m, &args.out).map_err(|e| e.0)?;
            if let Ok(d) = std::env::var("SMA4_DUMP_OBJ") {
                std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
                objects::dump_obj_vram(&m, std::path::Path::new(&d)).map_err(|e| e.0)?;
            }
            // Last: it overwrites part of OBJ VRAM.
            objects::extract_player(&rom, &mut m, &args.out).map_err(|e| e.0)?;
            // These reset the machine.
            screens::extract_map(&mut m, 0, &args.out).map_err(|e| e.0)?;
            worldmap::extract_world_map(&rom, &mut m, 0, &args.out).map_err(|e| e.0)?;
            worldmap::extract_item_panel(&rom, &mut m, &args.out).map_err(|e| e.0)?;
            // World 9, the Warp Zone the Warp Whistle leads to (0x03002A69 = 8).
            worldmap::extract_world_map(&rom, &mut m, WARP_ZONE, &args.out).map_err(|e| e.0)?;
            // World 9 has no walking map objects (its object table reads off the ROM).
            worldmap::extract_map_objects(&rom, &mut m, 0, &args.out).map_err(|e| e.0)?;
            screens::extract_game_over(rom.bytes(), &mut m, &args.out).map_err(|e| e.0)?;
            screens::extract_game_over_menu(&mut m, &args.out).map_err(|e| e.0)?;
            spade::extract(&rom, &mut m, &args.out).map_err(|e| e.0)?;
            title::extract(&rom, &mut m, &args.out).map_err(|e| e.0)?;
        }
        n += 1;
    }

    // After the castle: the letter (taken with the throne room) and the save screen.
    if let Some(letter) = letter {
        ending::extract(&rom, &mut m, &args.out, letter).map_err(|e| e.0)?;
    }
    // Worlds 2-8's maps: World 2 is where the game goes after World 1's castle, and World
    // 9's pipes lead to all of them (their levels are not extracted).
    if args.levels.is_empty() {
        for world in 1..WARP_ZONE {
            worldmap::extract_world_map(&rom, &mut m, world, &args.out).map_err(|e| e.0)?;
            worldmap::extract_map_objects(&rom, &mut m, world, &args.out).map_err(|e| e.0)?;
        }
    }

    let sheets = extract_gfx(&rom, args)?;
    println!("  {sheets} graphics sheets");

    let levels: Vec<String> = levels
        .iter()
        .map(|(id, ts, en)| format!("{id:#05x}:{ts}:{en:#04x}"))
        .collect();
    write_manifest(
        &args.out,
        &format!(
            "# Extracted from the user's own ROM by sma4-extract. Do not commit or share.\n\
             game={}\nversion={:?}\nlevels={}\ngfx_sheets={sheets}\n",
            h.game_code,
            h.version,
            levels.join(","),
        ),
    )
    .map_err(|e| e.to_string())?;
    println!("done");
    Ok(())
}

fn main() -> std::process::ExitCode {
    let Some(args) = parse_args() else {
        eprintln!("{USAGE}");
        return std::process::ExitCode::FAILURE;
    };
    match run(&args) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

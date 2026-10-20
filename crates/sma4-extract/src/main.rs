//! Extracts game assets from the user's own SMA4 ROM into a local directory
//! (default `assets/extracted/`). The game then runs from those files alone.
//!
//! The output is derived from the ROM: keep it on your machine, never commit or share it.

mod emu;
mod level;
mod oam;
mod objects;
mod screens;
mod worldmap;

use std::path::PathBuf;

use sma4_assets::{DEFAULT_DIR, Rgba, gfx_path, write_manifest};
use sma4_rom::{Rom, gfx};

const USAGE: &str = "usage: sma4-extract <rom.gba> [--out <dir>] [--level <id>:<tileset>:<enemies> ...]
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
    Some(Args { rom: rom?, out, levels })
}

/// Every LZ77 tile blob as a greyscale 4bpp sheet, for the in-game graphics viewer.
fn extract_gfx(rom: &Rom, args: &Args) -> Result<usize, String> {
    let grey: Vec<gfx::Bgr555> = (0..16u16).map(|i| gfx::Bgr555((i * 31 / 15) * 0x421)).collect();
    let blobs = rom.scan_lz77(256, gfx::TILE_BYTES_4BPP);
    for b in &blobs {
        let data = rom.decompress(b.addr).map_err(|e| e.to_string())?;
        let (pixels, w, h) = gfx::tile_sheet_rgba(&gfx::tiles_4bpp(&data), &grey, SHEET_COLS);
        Rgba { width: w as u32, height: h as u32, pixels }
            .write_png(&gfx_path(&args.out, b.addr))
            .map_err(|e| e.to_string())?;
    }
    Ok(blobs.len())
}

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
    // By default: every course level World 1's map can enter (1-1 first).
    let levels = if args.levels.is_empty() {
        worldmap::course_levels(&rom, &mut m, 0).map_err(|e| e.0)?
    } else {
        args.levels.clone()
    };
    let course_levels = levels.len();
    let mut levels = levels;
    let mut n = 0;
    while n < levels.len() {
        let (id, tileset, enemies) = levels[n];
        let asset = match level::extract(&rom, &mut m, id, tileset, enemies) {
            Ok(a) => a,
            // A pipe's area that does not load is skipped (its pipe then stays shut).
            Err(e) if n >= course_levels => {
                println!("  level {id:#x} ({:#010x}): {}, skipped", level::level_addr(&rom, id), e.0);
                levels.remove(n);
                continue;
            }
            Err(e) => return Err(format!("level {id:#x} ({:#010x}): {}", level::level_addr(&rom, id), e.0)),
        };
        asset.write(&args.out).map_err(|e| e.to_string())?;
        // Areas behind pipes (the header's alternate level) are extracted too.
        if let Some(j) = &asset.junction
            && !levels.iter().any(|l| l.0 == j.level)
        {
            levels.push((j.level, j.tileset, j.enemies));
        }
        // Object graphics are loaded by the same routines; take them after the first level.
        if n == 0 {
            objects::extract_coin(&rom, &m, &args.out).map_err(|e| e.0)?;
            objects::extract_walker(&rom, &m, &args.out).map_err(|e| e.0)?;
            objects::extract_sparkle(&m, &args.out).map_err(|e| e.0)?;
            objects::extract_skid_dust(&rom, &m, &args.out).map_err(|e| e.0)?;
            objects::extract_hud(&rom, &m, &args.out).map_err(|e| e.0)?;
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
            screens::extract_game_over(rom.bytes(), &mut m, &args.out).map_err(|e| e.0)?;
            screens::extract_game_over_menu(&mut m, &args.out).map_err(|e| e.0)?;
        }
        n += 1;
    }

    let sheets = extract_gfx(&rom, args)?;
    println!("  {sheets} graphics sheets");

    let levels: Vec<String> =
        levels.iter().map(|(id, ts, en)| format!("{id:#05x}:{ts}:{en:#04x}")).collect();
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

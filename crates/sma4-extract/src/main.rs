//! Extracts game assets from the user's own SMA4 ROM into a local directory
//! (default `assets/extracted/`). The game then runs from those files alone.
//!
//! The output is derived from the ROM: keep it on your machine, never commit or share it.

mod emu;
mod level;

use std::path::PathBuf;

use sma4_assets::{DEFAULT_DIR, Rgba, gfx_path, write_manifest};
use sma4_rom::{Rom, gfx};

const USAGE: &str = "usage: sma4-extract <rom.gba> [--out <dir>] [--level <id>:<tileset> ...]
  --out    output directory (default assets/extracted)
  --level  level to extract, hex id and tileset, repeatable (default 0x22:1 = World 1-1)";

/// World 1-1 (first entry of the World 1 map table at 0x08133AF4).
const DEFAULT_LEVELS: &[(u16, u8)] = &[(0x22, 1)];
const SHEET_COLS: usize = 32;

struct Args {
    rom: String,
    out: PathBuf,
    levels: Vec<(u16, u8)>,
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
                let (id, ts) = spec.split_once(':')?;
                let id = u16::from_str_radix(id.trim_start_matches("0x"), 16).ok()?;
                levels.push((id, ts.parse().ok()?));
            }
            "-h" | "--help" => return None,
            _ => rom = Some(a),
        }
    }
    if levels.is_empty() {
        levels = DEFAULT_LEVELS.to_vec();
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
    for &(id, tileset) in &args.levels {
        let asset = level::extract(&rom, &mut m, id, tileset).map_err(|e| {
            format!("level {id:#x} ({:#010x}): {}", level::level_addr(&rom, id), e.0)
        })?;
        asset.write(&args.out).map_err(|e| e.to_string())?;
    }

    let sheets = extract_gfx(&rom, args)?;
    println!("  {sheets} graphics sheets");

    let levels: Vec<String> = args.levels.iter().map(|(id, ts)| format!("{id:#05x}:{ts}")).collect();
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

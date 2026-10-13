//! Bevy host for the SMA4 port. Bevy handles windowing, input, rendering and audio;
//! all game rules live in `sma4-core` and run in `FixedUpdate` at the GBA frame rate.
//!
//! The game reads only extracted assets (default `assets/extracted/`, created once by
//! `sma4-extract` from the user's own ROM); it never opens the ROM itself.

mod enemies;
mod goal;
mod hud;
mod input;
mod level;
mod player;
mod screens;
mod viewer;
mod worldmap;

use std::path::PathBuf;

use bevy::{camera::ScalingMode, prelude::*};
use sma4_assets::{DEFAULT_DIR, EnemySpawn, LevelAsset, TileKind};
use sma4_core::{FRAME_HZ, Game, SCREEN_H, SCREEN_W};

/// Initial window size as a multiple of the GBA screen.
const SCALE: u32 = 4;

/// World 1-1.
const DEFAULT_LEVEL: u16 = 0x22;

const USAGE: &str = "usage: sma4 [--assets <dir>] [--level <hex id>] [--viewer]
  --assets  extracted asset directory (default assets/extracted)
  --level   level to play, hex id (default 22 = World 1-1)
  --viewer  browse the extracted graphics sheets instead of playing
  --spawn-col <n>  debug: start the player at metatile column n
  --place <hex type>@<col>  debug: add an enemy standing on the ground at a column
            (a Piranha Plant goes in the pipe there: column = the pipe's left column)
  --hit <col>,<row>  debug: hit the block at a cell from below when the level starts
  --form <0|1|3>  debug: start small, big or raccoon
  --no-enemies  debug: leave the level's enemy list out
  --map  start on the world map even with debug options / scripts
  --lives <n>  debug: start with n lives
  --cleared <col>,<row>  debug: mark the world-map spot at that cell cleared (repeatable)
  --popup <type>  debug: show a score popup (1-15) over the player at the start
  --script <spec> --screenshot <png>
            play scripted input (e.g. \"-30 R120 RA20 R60\"), save a screenshot, exit

Assets come from your own ROM, extracted once with:
  cargo run --release -p sma4-extract -- <rom.gba>";

/// Directory the extracted assets are read from.
#[derive(Resource, Clone)]
pub struct AssetDir(pub PathBuf);

#[derive(Resource, Default)]
pub struct GameRes(pub Game);

struct Args {
    assets: PathBuf,
    level: u16,
    viewer: bool,
    script: Option<String>,
    screenshot: String,
    spawn_col: Option<i32>,
    place: Vec<(u16, u16)>,
    hit: Option<(i32, i32)>,
    form: u8,
    no_enemies: bool,
    popup: Option<u8>,
    map: bool,
    lives: Option<u16>,
    cleared: Vec<(usize, usize)>,
}

fn parse_args() -> Option<Args> {
    let mut args = Args {
        assets: PathBuf::from(DEFAULT_DIR),
        level: DEFAULT_LEVEL,
        viewer: false,
        script: None,
        screenshot: "screenshot.png".into(),
        spawn_col: None,
        place: Vec::new(),
        hit: None,
        form: 0,
        no_enemies: false,
        popup: None,
        map: false,
        lives: None,
        cleared: Vec::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--assets" => args.assets = it.next()?.into(),
            "--level" => args.level = u16::from_str_radix(it.next()?.trim_start_matches("0x"), 16).ok()?,
            "--viewer" => args.viewer = true,
            "--script" => args.script = Some(it.next()?),
            "--screenshot" => args.screenshot = it.next()?,
            "--spawn-col" => args.spawn_col = Some(it.next()?.parse().ok()?),
            "--no-enemies" => args.no_enemies = true,
            "--map" => args.map = true,
            "--lives" => args.lives = Some(it.next()?.parse().ok()?),
            "--cleared" => {
                let spec = it.next()?;
                let (c, r) = spec.split_once(',')?;
                args.cleared.push((c.parse().ok()?, r.parse().ok()?));
            }
            "--popup" => args.popup = Some(it.next()?.parse().ok()?),
            "--form" => args.form = it.next()?.parse().ok()?,
            "--hit" => {
                let spec = it.next()?;
                let (x, y) = spec.split_once(',')?;
                args.hit = Some((x.parse().ok()?, y.parse().ok()?));
            }
            "--place" => {
                let spec = it.next()?;
                let (kind, col) = spec.split_once('@')?;
                args.place.push((u16::from_str_radix(kind.trim_start_matches("0x"), 16).ok()?, col.parse().ok()?));
            }
            _ => return None,
        }
    }
    Some(args)
}

fn main() -> AppExit {
    let Some(args) = parse_args() else {
        eprintln!("{USAGE}");
        return AppExit::error();
    };
    if !args.assets.join("manifest.txt").exists() {
        eprintln!("no extracted assets in {}\n\n{USAGE}", args.assets.display());
        return AppExit::error();
    }

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(ImagePlugin::default_nearest())
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "SMA4 port".into(),
                    resolution: (SCREEN_W * SCALE, SCREEN_H * SCALE).into(),
                    ..default()
                }),
                ..default()
            }),
    )
    .insert_resource(Time::<Fixed>::from_hz(FRAME_HZ))
    .insert_resource(AssetDir(args.assets.clone()))
    .init_resource::<GameRes>()
    .add_systems(Startup, spawn_camera);

    if args.viewer {
        app.add_plugins(viewer::ViewerPlugin);
    } else {
        let mut level = match LevelAsset::read(&args.assets, args.level) {
            Ok(l) => l,
            Err(e) => {
                eprintln!(
                    "{e}\nextract it with: cargo run --release -p sma4-extract -- <rom.gba> --level {:#x}:<tileset>",
                    args.level
                );
                return AppExit::error();
            }
        };
        if args.no_enemies {
            level.enemies.clear();
        }
        for &(kind, col) in &args.place {
            // Standing on the lowest surface of the column.
            let c = col as usize;
            let row = (1..level.height)
                .rev()
                .find(|&y| level.kind(c, y) != TileKind::Empty && level.kind(c, y - 1) == TileKind::Empty)
                .unwrap_or(1);
            // Piranha Plants are listed at the top row of their pipe.
            let row = if sma4_core::enemies::PIRANHA_TYPES.contains(&kind) { row } else { row - 1 };
            level.enemies.push(EnemySpawn { kind, col, row: row as u8 });
        }
        if let Some(spec) = &args.script {
            let Some(script) = input::Script::parse(spec, args.screenshot.clone()) else {
                eprintln!("bad --script {spec:?}\n{USAGE}");
                return AppExit::error();
            };
            app.insert_resource(script);
        }
        app.insert_resource(level::SpawnColumn(args.spawn_col))
            .insert_resource(level::DebugPopup(args.popup))
            // A game starts on the world map; scripted and debug runs go straight in.
            .insert_resource(level::StartOptions {
                on_map: args.map
                    || (args.script.is_none() && args.spawn_col.is_none() && args.hit.is_none() && args.form == 0),
                hit: args.hit,
                form: args.form,
                lives: args.lives,
            })
            .insert_resource(level::LevelRes(level))
            .insert_resource(worldmap::ClearedSpots(args.cleared.clone()))
            .add_plugins((
                input::InputPlugin,
                level::LevelPlugin,
                player::PlayerPlugin,
                enemies::EnemyPlugin,
                goal::GoalPlugin,
                hud::HudPlugin,
                screens::ScreensPlugin,
                worldmap::WorldMapPlugin,
            ));
    }
    app.run()
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        Msaa::Off,
        // Always show (at least) the GBA's 240x160, whatever size the window ends up.
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::AutoMin { min_width: SCREEN_W as f32, min_height: SCREEN_H as f32 },
            ..OrthographicProjection::default_2d()
        }),
    ));
}

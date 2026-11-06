//! Bevy host for the SMA4 port. Bevy handles windowing, input, rendering and audio;
//! all game rules live in `sma4-core`, stepped once per vblank when the display runs near the
//! GBA's frame rate (`pacing`).
//!
//! The game reads only extracted assets (default `assets/extracted/`, created once by
//! `sma4-extract` from the user's own ROM); it never opens the ROM itself.

mod enemies;
mod fortress;
mod goal;
mod toad_house;
mod hud;
mod input;
mod level;
mod pacing;
mod player;
mod screens;
mod spade;
mod title;
mod viewer;
mod worldmap;

use std::path::PathBuf;

use bevy::{camera::ScalingMode, prelude::*};
use sma4_assets::{DEFAULT_DIR, EnemySpawn, LevelAsset, TileKind};
use sma4_core::{Game, SCREEN_H, SCREEN_W};

/// Initial window size as a multiple of the GBA screen.
const SCALE: u32 = 4;

/// World 1-1.
const DEFAULT_LEVEL: u16 = 0x22;

const USAGE: &str = "usage: sma4 [--assets <dir>] [--level <hex id>] [--viewer]
  --assets  extracted asset directory (default assets/extracted)
  --level   level to play, hex id (default 22 = World 1-1)
  --viewer  browse the extracted graphics sheets instead of playing
  --spawn-col <n>  debug: start the player at metatile column n
  --spawn-row <n>  debug: with --spawn-col, stand on metatile row n (not the lowest surface),
                   centred on the column's right edge (the middle of a pipe starting there)
  --place <hex type>@<col>  debug: add an enemy standing on the ground at a column
            (a Piranha Plant goes in the pipe there: column = the pipe's left column)
  --hit <col>,<row>  debug: hit the block at a cell from below when the level starts
  --form <0-3>  debug: start small, big, fire or raccoon
  --behind  debug: start behind the scenery (as just through a white block)
  --no-enemies  debug: leave the level's enemy list out
  --map  start on the world map even with debug options / scripts
  --lives <n>  debug: start with n lives
  --progress <n>  debug: the game's progress byte (not 0 once beaten; the Advance Coin appears)
  --items <hex,...>  debug: start with these items (1 mushroom, 2 flower, 3 leaf, 9 star,
                c Warp Whistle; L on the map opens the panel)
  --cleared <col>,<row>  debug: mark the world-map spot at that cell cleared (repeatable)
  --map-bro <col>,<row>  debug: put the world map's Hammer Bro on that cell
  --map-at <col>,<row>  debug: put Mario on that world-map cell (e.g. 12,3: a Toad House)
  --popup <type>  debug: show a score popup (1-15) over the player at the start
  --title  debug: start on the title screen (where the game-over menu's QUIT goes)
  --spade  debug: start in the spade panel's game (Toad's room)
  --streak <n>  debug: the spade streak count so far (0-7: the bonus rounds from 3; the map's
                spade panel shows a heart, club or diamond from 1)
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
    spawn_row: Option<i32>,
    place: Vec<(u16, u16)>,
    hit: Option<(i32, i32)>,
    form: u8,
    behind: bool,
    no_enemies: bool,
    popup: Option<u8>,
    map: bool,
    lives: Option<u16>,
    progress: u8,
    items: Vec<u8>,
    cleared: Vec<(usize, usize)>,
    map_bro: Option<(usize, usize)>,
    map_at: Option<(usize, usize)>,
    spade: bool,
    streak: Option<u8>,
    title: bool,
}

fn parse_args() -> Option<Args> {
    let mut args = Args {
        assets: PathBuf::from(DEFAULT_DIR),
        level: DEFAULT_LEVEL,
        viewer: false,
        script: None,
        screenshot: "screenshot.png".into(),
        spawn_col: None,
        spawn_row: None,
        place: Vec::new(),
        hit: None,
        form: 0,
        behind: false,
        no_enemies: false,
        popup: None,
        map: false,
        lives: None,
        progress: 0,
        items: Vec::new(),
        cleared: Vec::new(),
        map_bro: None,
        map_at: None,
        spade: false,
        streak: None,
        title: false,
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
            "--spawn-row" => args.spawn_row = Some(it.next()?.parse().ok()?),
            "--no-enemies" => args.no_enemies = true,
            "--map" => args.map = true,
            "--lives" => args.lives = Some(it.next()?.parse().ok()?),
            "--progress" => args.progress = it.next()?.parse().ok()?,
            "--items" => {
                args.items = it.next()?.split(',').map(|h| u8::from_str_radix(h, 16).ok()).collect::<Option<_>>()?;
            }
            "--map-at" => {
                let v = it.next()?;
                let (c, r) = v.split_once(',')?;
                args.map_at = Some((c.parse().ok()?, r.parse().ok()?));
            }
            "--map-bro" => {
                let v = it.next()?;
                let (c, r) = v.split_once(',')?;
                args.map_bro = Some((c.parse().ok()?, r.parse().ok()?));
            }
            "--cleared" => {
                let spec = it.next()?;
                let (c, r) = spec.split_once(',')?;
                args.cleared.push((c.parse().ok()?, r.parse().ok()?));
            }
            "--popup" => args.popup = Some(it.next()?.parse().ok()?),
            "--spade" => args.spade = true,
            "--title" => args.title = true,
            "--streak" => args.streak = Some(it.next()?.parse().ok()?),
            "--form" => args.form = it.next()?.parse().ok()?,
            "--behind" => args.behind = true,
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
                    // Scripted runs must not wait on vsync: a hidden window never gets it.
                    present_mode: if args.script.is_some() {
                        bevy::window::PresentMode::AutoNoVsync
                    } else {
                        bevy::window::PresentMode::default()
                    },
                    ..default()
                }),
                ..default()
            }),
    )
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
            level.enemies.push(EnemySpawn { kind, col, row: row as u8, ..Default::default() });
        }
        if let Some(spec) = &args.script {
            let Some(script) = input::Script::parse(spec, args.screenshot.clone()) else {
                eprintln!("bad --script {spec:?}\n{USAGE}");
                return AppExit::error();
            };
            // Keep stepping when the window is not focused (Bevy slows unfocused windows).
            app.insert_resource(script).insert_resource(bevy::winit::WinitSettings::continuous());
        }
        app.insert_resource(level::SpawnColumn(args.spawn_col, args.spawn_row))
            .insert_resource(level::DebugPopup(args.popup))
            // A game starts on the world map; scripted and debug runs go straight in.
            .insert_resource(level::StartOptions {
                on_map: args.map
                    || (args.script.is_none() && args.spawn_col.is_none() && args.hit.is_none() && args.form == 0),
                hit: args.hit,
                form: args.form,
                lives: args.lives,
                behind: args.behind,
                progress: args.progress,
                items: args.items.clone(),
            })
            .insert_resource(level::LevelRes(level))
            .insert_resource(worldmap::ClearedSpots(args.cleared.clone()))
            .insert_resource(worldmap::MapBro(args.map_bro))
            .insert_resource(worldmap::MapAt(args.map_at))
            .insert_resource(spade::DebugSpade(args.spade.then_some(args.streak.unwrap_or(0))))
            .insert_resource(spade::DebugStreak(args.streak))
            .insert_resource(title::DebugTitle(args.title))
            .add_plugins((
                input::InputPlugin,
                level::LevelPlugin,
                player::PlayerPlugin,
                enemies::EnemyPlugin,
                fortress::FortressPlugin,
                goal::GoalPlugin,
                toad_house::ToadHousePlugin,
                hud::HudPlugin,
                screens::ScreensPlugin,
                worldmap::WorldMapPlugin,
                spade::SpadePlugin,
                title::TitlePlugin,
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

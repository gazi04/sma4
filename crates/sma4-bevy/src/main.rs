//! Bevy host for the SMA4 port. Bevy handles windowing, input, rendering and audio;
//! all game rules live in `sma4-core` and run in `FixedUpdate` at the GBA frame rate.
//!
//! The game reads only extracted assets (default `assets/extracted/`, created once by
//! `sma4-extract` from the user's own ROM); it never opens the ROM itself.

mod input;
mod level;
mod player;
mod viewer;

use std::path::PathBuf;

use bevy::{camera::ScalingMode, prelude::*};
use sma4_assets::{DEFAULT_DIR, LevelAsset};
use sma4_core::{FRAME_HZ, Game, SCREEN_H, SCREEN_W};

/// Initial window size as a multiple of the GBA screen.
const SCALE: u32 = 4;

/// World 1-1.
const DEFAULT_LEVEL: u16 = 0x22;

const USAGE: &str = "usage: sma4 [--assets <dir>] [--level <hex id>] [--viewer]
  --assets  extracted asset directory (default assets/extracted)
  --level   level to play, hex id (default 22 = World 1-1)
  --viewer  browse the extracted graphics sheets instead of playing
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
}

fn parse_args() -> Option<Args> {
    let mut args = Args {
        assets: PathBuf::from(DEFAULT_DIR),
        level: DEFAULT_LEVEL,
        viewer: false,
        script: None,
        screenshot: "screenshot.png".into(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--assets" => args.assets = it.next()?.into(),
            "--level" => args.level = u16::from_str_radix(it.next()?.trim_start_matches("0x"), 16).ok()?,
            "--viewer" => args.viewer = true,
            "--script" => args.script = Some(it.next()?),
            "--screenshot" => args.screenshot = it.next()?,
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
        let level = match LevelAsset::read(&args.assets, args.level) {
            Ok(l) => l,
            Err(e) => {
                eprintln!(
                    "{e}\nextract it with: cargo run --release -p sma4-extract -- <rom.gba> --level {:#x}:<tileset>",
                    args.level
                );
                return AppExit::error();
            }
        };
        if let Some(spec) = &args.script {
            let Some(script) = input::Script::parse(spec, args.screenshot.clone()) else {
                eprintln!("bad --script {spec:?}\n{USAGE}");
                return AppExit::error();
            };
            app.insert_resource(script);
        }
        app.insert_resource(level::LevelRes(level))
            .add_plugins((input::InputPlugin, level::LevelPlugin, player::PlayerPlugin));
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

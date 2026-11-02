use std::collections::VecDeque;

use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};
use sma4_core::Buttons;

use crate::{GameRes, pacing::Pacer};

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        // After input (PreUpdate), before everything that draws the game (Update): where
        // `FixedUpdate` would run.
        app.init_resource::<Pacing>()
            .add_systems(RunFixedMainLoop, step_game.in_set(RunFixedMainLoopSystems::FixedMainLoop))
            .add_systems(Update, finish_script);
    }
}

const KEYMAP: [(KeyCode, Buttons); 10] = [
    (KeyCode::KeyX, Buttons::A),
    (KeyCode::KeyZ, Buttons::B),
    (KeyCode::Backspace, Buttons::SELECT),
    (KeyCode::Enter, Buttons::START),
    (KeyCode::ArrowRight, Buttons::RIGHT),
    (KeyCode::ArrowLeft, Buttons::LEFT),
    (KeyCode::ArrowUp, Buttons::UP),
    (KeyCode::ArrowDown, Buttons::DOWN),
    (KeyCode::KeyS, Buttons::R),
    (KeyCode::KeyA, Buttons::L),
];

fn held_buttons(keys: &ButtonInput<KeyCode>) -> Buttons {
    let mut b = Buttons::empty();
    for (key, button) in KEYMAP {
        if keys.pressed(key) {
            b |= button;
        }
    }
    b
}

/// Scripted input for automated checks: replaces the keyboard, then takes a screenshot
/// and exits. Format: whitespace-separated `<buttons><frames>`, buttons from
/// `R L U D A B` (or `-` for none), e.g. `-30 R120 RA20 R60`.
#[derive(Resource)]
pub struct Script {
    steps: VecDeque<(Buttons, u32)>,
    screenshot: String,
    state: ScriptState,
}

#[derive(Clone, Copy, PartialEq)]
enum ScriptState {
    Running,
    Captured(u32),
}

impl Script {
    pub fn parse(spec: &str, screenshot: String) -> Option<Self> {
        let mut steps = VecDeque::new();
        for tok in spec.split_whitespace() {
            let split = tok.find(|c: char| c.is_ascii_digit())?;
            let (keys, frames) = tok.split_at(split);
            let mut b = Buttons::empty();
            for c in keys.chars() {
                b |= match c {
                    'R' => Buttons::RIGHT,
                    'L' => Buttons::LEFT,
                    'U' => Buttons::UP,
                    'D' => Buttons::DOWN,
                    'A' => Buttons::A,
                    'B' => Buttons::B,
                    // The shoulder buttons (L and R are taken by the pad).
                    'Q' => Buttons::L,
                    'W' => Buttons::R,
                    '-' => Buttons::empty(),
                    _ => return None,
                };
            }
            steps.push_back((b, frames.parse().ok()?));
        }
        Some(Self { steps, screenshot, state: ScriptState::Running })
    }

    fn next(&mut self) -> Option<Buttons> {
        let (b, n) = self.steps.front_mut()?;
        let b = *b;
        *n -= 1;
        if *n == 0 {
            self.steps.pop_front();
        }
        Some(b)
    }
}

#[derive(Resource, Default)]
struct Pacing(Pacer);

/// Runs the core frames due this rendered frame (`Pacer`); the core never sees real time.
/// A script runs one frame per rendered frame, so it plays the same however fast it renders.
fn step_game(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    mut pacing: ResMut<Pacing>,
    script: Option<ResMut<Script>>,
    mut game: ResMut<GameRes>,
) {
    if let Some(mut s) = script {
        // Script over: freeze so the screenshot shows the final frame.
        if let Some(held) = s.next() {
            game.0.step(held);
        }
        return;
    }
    let held = held_buttons(&keys);
    let was_locked = pacing.0.locked();
    let n = pacing.0.steps(time.delta_secs_f64());
    if pacing.0.locked() != was_locked {
        let hz = 1.0 / pacing.0.period();
        info!("display {hz:.3} Hz: {}", if pacing.0.locked() { "one game frame per vblank" } else { "fixed timestep" });
    }
    for _ in 0..n {
        game.0.step(held);
    }
}

fn finish_script(mut commands: Commands, script: Option<ResMut<Script>>, game: Res<GameRes>, mut exit: MessageWriter<AppExit>) {
    let Some(mut s) = script else { return };
    let done = s.steps.is_empty();
    match s.state {
        ScriptState::Running if done => {
            let p = &game.0.player;
            let enemies: Vec<_> = game.0.enemies.iter().map(|e| (e.px(), e.state)).collect();
            println!(
                "script done at frame {}: player px={:?} v=({}, {}) on_ground={} form={:?} invulnerable={} enemies={:?} coins={} score={} lives={} clock={} screen={:?} dying={:?} popups={:?} sparkles={:?} items={:?} bumps={:?} goal={:?} cards={:?}",
                game.0.frame,
                p.px(),
                p.vx,
                p.vy,
                p.on_ground,
                p.form,
                game.0.invulnerable,
                enemies,
                game.0.status.coins,
                game.0.status.score * 10,
                game.0.status.lives.wrapping_add(1),
                game.0.status.seconds(),
                game.0.screen,
                game.0.dying,
                game.0.status.popups,
                game.0.sparkles,
                game.0.items.iter().map(|i| i.px()).collect::<Vec<_>>(),
                game.0.bumps.iter().map(|b| (b.tx, b.ty, b.timer)).collect::<Vec<_>>(),
                game.0.goal.map(|g| (g.px(), g.phase, g.card, g.timer)),
                game.0.status.cards
            );
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(s.screenshot.clone()));
            s.state = ScriptState::Captured(0);
        }
        // Give the screenshot a few frames to be written before exiting.
        ScriptState::Captured(n) if n > 30 => {
            exit.write(AppExit::Success);
        }
        ScriptState::Captured(n) => s.state = ScriptState::Captured(n + 1),
        ScriptState::Running => {}
    }
}

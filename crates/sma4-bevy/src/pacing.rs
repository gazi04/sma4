//! How many core frames to run per rendered frame.
//!
//! The GBA runs at 59.7275 Hz (`FRAME_HZ`); displays run near 60 Hz but never exactly at it.
//! A fixed timestep at the GBA rate beats against the display: every few seconds a rendered
//! frame gets no step (the picture freezes) and the next gets two, and while the timestep's
//! phase sits on the edge of a vblank the ordinary jitter in frame times does that for many
//! frames in a row. Emulators avoid this by locking the game to the display: one frame per
//! vblank, a fraction of a percent fast. This does that when the display is close enough to
//! the GBA's rate, and falls back to the fixed timestep otherwise (other refresh rates,
//! no vsync).

use std::collections::VecDeque;

use sma4_core::FRAME_HZ;

const GBA_PERIOD: f64 = 1.0 / FRAME_HZ;
/// Frames the display period is measured over (4 s).
const WINDOW: usize = 240;
/// Frames measured before locking at all.
const WARM_UP: usize = 60;
/// Lock when the display is within 5% of the GBA's rate; unlock beyond 8%. Only ~60 Hz has
/// to be told from the next common rates (50, 75, 120, 144 Hz, no vsync), and the measured
/// rate wobbles by a few percent.
const LOCK: f64 = 0.05;
const UNLOCK: f64 = 0.08;
/// At most this many core frames per rendered frame (after a long stall the game skips time
/// rather than racing to catch up).
const MAX_STEPS: u32 = 4;

#[derive(Default)]
pub struct Pacer {
    /// Recent frame times.
    window: VecDeque<f64>,
    /// The display's frame period, measured.
    period: Option<f64>,
    locked: bool,
    /// Fixed-timestep time not yet stepped (only while unlocked).
    acc: f64,
}

impl Pacer {
    /// The display's frame period: the median of the window's frame times. A frame time is
    /// noisy (taken when the frame starts, not at the vblank), and stalls (loading, the
    /// compositor) and the short frames catching up after them come in uneven lengths; the
    /// median ignores both.
    pub fn period(&self) -> f64 {
        self.period.unwrap_or(GBA_PERIOD)
    }

    pub fn locked(&self) -> bool {
        self.locked
    }

    /// Core frames to run for a rendered frame that took `dt` seconds.
    pub fn steps(&mut self, dt: f64) -> u32 {
        self.window.push_back(dt);
        if self.window.len() > WINDOW {
            self.window.pop_front();
        }
        if self.window.len() >= WARM_UP {
            let mut sorted: Vec<f64> = self.window.iter().copied().collect();
            sorted.sort_by(f64::total_cmp);
            self.period = Some(sorted[sorted.len() / 2]);
        }
        let period = self.period();
        // With vsync every rendered frame is at least one vblank, however short it measured;
        // a frame of 1.5 periods or more missed vblanks.
        let vblanks = if dt >= 1.5 * period { (dt / period).round() as u32 } else { 1 };

        let off = (period / GBA_PERIOD - 1.0).abs();
        if self.locked && off > UNLOCK {
            self.locked = false;
        } else if !self.locked && off < LOCK && self.period.is_some() {
            self.locked = true;
            self.acc = 0.0;
        }

        if self.locked {
            return vblanks.min(MAX_STEPS);
        }
        self.acc += dt;
        let n = (self.acc / GBA_PERIOD) as u32;
        if n > MAX_STEPS {
            self.acc = 0.0;
            return MAX_STEPS;
        }
        self.acc -= n as f64 * GBA_PERIOD;
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frame times at `hz`, each off by up to `jitter` seconds (a fixed pseudo-random pattern
    /// whose errors cancel out, as when the start-of-frame time wobbles around the vblank).
    fn frames(hz: f64, jitter: f64, n: usize) -> Vec<f64> {
        let mut seed = 0x1234_5678u32;
        let mut offsets = vec![0.0];
        for _ in 0..n {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            offsets.push((seed >> 8) as f64 / (1u32 << 24) as f64 * 2.0 * jitter - jitter);
        }
        offsets.windows(2).map(|o| 1.0 / hz + o[1] - o[0]).collect()
    }

    #[test]
    fn near_sixty_hz_runs_one_step_every_frame() {
        for hz in [60.0, 59.951, 60.003, FRAME_HZ] {
            let mut p = Pacer::default();
            let steps: Vec<u32> = frames(hz, 0.004, 2000).into_iter().map(|dt| p.steps(dt)).collect();
            assert!(p.locked(), "{hz} Hz locks");
            assert!(steps[WARM_UP..].iter().all(|&n| n == 1), "{hz} Hz: one step per frame");
        }
    }

    #[test]
    fn a_missed_vblank_runs_the_frames_it_spanned() {
        let mut p = Pacer::default();
        for dt in frames(60.0, 0.002, 200) {
            p.steps(dt);
        }
        assert_eq!(p.steps(2.0 / 60.0), 2);
        assert_eq!(p.steps(3.0 / 60.0), 3);
        assert_eq!(p.steps(1.0), MAX_STEPS);
        assert!(p.locked(), "one stall does not unlock");
    }

    #[test]
    fn stalls_do_not_skew_the_measured_rate() {
        let mut p = Pacer::default();
        // Start-up: long, uneven frames among the vsynced ones.
        for (i, dt) in frames(59.951, 0.004, 400).into_iter().enumerate() {
            p.steps(if i % 25 == 3 { 0.042 + i as f64 * 1e-4 } else { dt });
        }
        assert!(p.locked());
        assert!((1.0 / p.period() - 59.951).abs() < 1.0, "measured {} Hz", 1.0 / p.period());
    }

    #[test]
    fn other_refresh_rates_keep_the_gba_rate() {
        for hz in [50.0, 75.0, 120.0, 144.0, 1000.0] {
            let mut p = Pacer::default();
            let dts = frames(hz, 0.0005, 144 * 20);
            let total: u32 = dts.iter().map(|&dt| p.steps(dt)).sum();
            let secs: f64 = dts.iter().sum();
            assert!(!p.locked(), "{hz} Hz does not lock");
            assert!((total as f64 - secs * FRAME_HZ).abs() <= 1.0, "{hz} Hz: {total} steps in {secs} s");
        }
    }
}

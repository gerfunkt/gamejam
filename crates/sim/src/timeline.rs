use crate::flight::Parts;
use glam::{Quat, Vec3};
use std::collections::VecDeque;

/// One state of a remote plane, as it arrived.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// The sender's clock at send time, in seconds.
    pub send: f64,
    pub pos: Vec3,
    pub vel: Vec3,
    pub rot: Quat,
    pub parts: Parts,
}

const OFFSET_WINDOW: f64 = 4.0;
const MIN_DELAY: f64 = 0.05;
const MAX_DELAY: f64 = 0.4;
const BASE_DELAY: f64 = 0.02;
const ADAPT_RATE: f64 = 0.05;
const COAST_S: f64 = 0.25;
const MAX_SAMPLES: usize = 64;

/// Snapshot interpolation for one remote plane. The receiver draws the
/// plane `interval + 2.5 x jitter + 20 ms` in the past, between two real
/// states, so bunched delivery never moves it backward.
pub struct Timeline {
    samples: VecDeque<Sample>,
    arrivals: VecDeque<(f64, f64)>, // (recv, travel) within the window
    delay: f64,
    render_time: f64,
    last_now: f64,
    interval: f64,
    started: bool,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            samples: VecDeque::new(),
            arrivals: VecDeque::new(),
            delay: MIN_DELAY,
            render_time: 0.0,
            last_now: 0.0,
            interval: 1.0 / crate::tuning::STATE_HZ as f64,
            started: false,
        }
    }
}

impl Timeline {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, recv: f64, sample: Sample) {
        self.samples.push_back(sample);
        while self.samples.len() > MAX_SAMPLES {
            self.samples.pop_front();
        }
        self.arrivals.push_back((recv, recv - sample.send));
        while self
            .arrivals
            .front()
            .is_some_and(|(t, _)| recv - t > OFFSET_WINDOW)
        {
            self.arrivals.pop_front();
        }
    }

    /// The fastest travel time in the window: the clock offset.
    fn offset(&self) -> f64 {
        self.arrivals
            .iter()
            .map(|(_, travel)| *travel)
            .fold(f64::INFINITY, f64::min)
    }

    fn jitter(&self) -> f64 {
        let offset = self.offset();
        if !offset.is_finite() {
            return 0.0;
        }
        let n = self.arrivals.len() as f64;
        self.arrivals
            .iter()
            .map(|(_, travel)| (travel - offset).abs())
            .sum::<f64>()
            / n
    }

    /// The delay applied right now.
    pub fn delay(&self) -> f64 {
        self.delay
    }

    /// The interpolated state for drawing at local time `now`.
    /// Returns None until the first sample arrives.
    pub fn sample(&mut self, now: f64) -> Option<Sample> {
        let newest = *self.samples.back()?;
        if !self.started {
            self.started = true;
            self.last_now = now;
            self.render_time = newest.send;
            return Some(newest);
        }
        let elapsed = (now - self.last_now).max(0.0);
        self.last_now = now;

        // The delay follows the measured need at 5 % of the elapsed
        // time, so the motion stays smooth.
        let target =
            (self.interval + 2.5 * self.jitter() + BASE_DELAY).clamp(MIN_DELAY, MAX_DELAY);
        self.delay += (target - self.delay) * (ADAPT_RATE * elapsed).min(1.0);
        self.delay = self.delay.clamp(MIN_DELAY, MAX_DELAY);

        // The render clock only moves forward: the plane never replays
        // motion. It coasts at most COAST_S past the newest sample, and
        // jumps forward if it falls behind the oldest.
        self.render_time += elapsed;
        self.render_time = self.render_time.min(newest.send + COAST_S);
        if let Some(oldest) = self.samples.front() {
            self.render_time = self.render_time.max(oldest.send);
        }
        Some(self.interpolate(self.render_time))
    }

    fn interpolate(&self, t: f64) -> Sample {
        let newest = *self.samples.back().unwrap();
        if t >= newest.send {
            let over = (t - newest.send).min(COAST_S) as f32;
            return Sample {
                pos: newest.pos + newest.vel * over,
                ..newest
            };
        }
        let mut older = self.samples[0];
        let mut newer = newest;
        for w in self.samples.as_slices().0.windows(2) {
            if w[0].send <= t && t <= w[1].send {
                older = w[0];
                newer = w[1];
                break;
            }
        }
        if t <= older.send {
            return older;
        }
        let span = (newer.send - older.send).max(1e-6);
        let k = ((t - older.send) / span) as f32;
        Sample {
            send: t,
            pos: older.pos.lerp(newer.pos, k),
            vel: newer.vel,
            rot: older.rot.slerp(newer.rot, k),
            parts: newer.parts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flight::Parts;

    fn state(send: f64, x: f32) -> Sample {
        Sample {
            send,
            pos: Vec3::new(x, 0.0, 0.0),
            vel: Vec3::new(30.0, 0.0, 0.0),
            rot: Quat::IDENTITY,
            parts: Parts::default(),
        }
    }

    /// Feed states sent every 33 ms that arrive in bunches, as a VPN
    /// over TCP delivers them, and check the drawn plane never steps
    /// backward along its heading.
    #[test]
    fn bunched_delivery_never_steps_backward() {
        let mut tl = Timeline::new();
        let dt = 1.0 / 60.0;
        let mut now = 0.0;
        let mut sent = 0.0;
        let mut last_x = f32::NEG_INFINITY;
        let mut queue: Vec<Sample> = Vec::new();
        for _ in 0..(40.0 / dt) as usize {
            now += dt;
            while sent <= now {
                queue.push(state(sent, (sent * 30.0) as f32));
                sent += 1.0 / 30.0;
            }
            // Deliver in bunches: everything queued, every ~120 ms.
            if (now * 8.0).fract() < dt * 8.0 {
                for s in queue.drain(..) {
                    tl.push(now, s);
                }
            }
            if let Some(s) = tl.sample(now) {
                assert!(
                    s.pos.x >= last_x - 1e-4,
                    "stepped backward: {last_x} -> {}",
                    s.pos.x
                );
                last_x = s.pos.x;
            }
        }
        assert!(last_x > 1000.0, "the plane should move on: {last_x}");
    }

    #[test]
    fn the_delay_settles_between_min_and_max() {
        let mut tl = Timeline::new();
        let mut now = 0.0;
        for i in 0..120 {
            now += 1.0 / 30.0;
            let travel = 0.035 + (i % 3) as f64 * 0.02; // 35..75 ms
            tl.push(now, state(now - travel, i as f32));
            tl.sample(now);
        }
        assert!(tl.delay() >= MIN_DELAY);
        assert!(tl.delay() <= MAX_DELAY);
    }

    #[test]
    fn a_stalled_sender_coasts_then_holds() {
        let mut tl = Timeline::new();
        let mut now = 0.0;
        for i in 0..30 {
            now += 1.0 / 30.0;
            tl.push(now, state(now, i as f32));
            tl.sample(now);
        }
        // The sender stalls for a second; the plane coasts briefly and stops.
        let mut last: Option<Vec3> = None;
        for _ in 0..60 {
            now += 1.0 / 60.0;
            if let Some(s) = tl.sample(now) {
                if let Some(prev) = last {
                    let moved = (s.pos - prev).length();
                    assert!(moved <= 30.0 / 60.0 + 1e-4, "coasted too fast: {moved}");
                }
                last = Some(s.pos);
            }
        }
        let first = last.unwrap();
        now += 2.0;
        let second = tl.sample(now).unwrap().pos;
        assert!(
            (second - first).length() < 1e-3,
            "after {COAST_S} s of coasting the plane holds: {first} -> {second}"
        );
    }

    #[test]
    fn a_slower_route_raises_the_delay() {
        let mut tl = Timeline::new();
        let mut now = 0.0;
        for i in 0..30 {
            now += 1.0 / 30.0;
            tl.push(now, state(now - 0.04, i as f32));
            tl.sample(now);
        }
        let low = tl.delay();
        for i in 30..300 {
            now += 1.0 / 30.0;
            tl.push(now, state(now - 0.2, i as f32));
            tl.sample(now);
        }
        assert!(
            tl.delay() > low,
            "delay did not adapt: {low} -> {}",
            tl.delay()
        );
    }
}

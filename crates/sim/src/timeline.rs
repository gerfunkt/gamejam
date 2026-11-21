//! Smooth motion for a plane that another machine flies.
//!
//! States arrive about 30 times a second, but a network delivers them unevenly: late, or in
//! bunches. Each state carries the clock of its sender, so the timeline knows when the sender
//! made it. The timeline shows the plane a short time in the past, between two real states.
//! That time follows the unevenness of the network, and it changes only slowly, so the plane
//! never replays a motion. When no newer state arrived, the plane coasts on its heading briefly.

use std::collections::VecDeque;

use crate::{tuning, wire::PlaneState};

/// The states of one remote plane, and the clock that shows them. All times are in milliseconds.
#[derive(Clone, Debug, Default)]
pub struct Timeline {
	/// Each state with the time at which its sender made it, on the clock of the sender.
	states: VecDeque<(f64, PlaneState)>,
	/// The arrival and the travel time of each state of the last seconds.
	travels: VecDeque<(f64, f64)>,
	/// The lowest travel time of the last seconds. It holds the difference of the two clocks.
	/// The rest of a travel time is waiting.
	offset: f64,
	/// The average waiting of a state beyond the lowest travel time.
	jitter: f64,
	/// How far behind the clock of this machine the plane shows: the offset and the delay.
	lag: f64,
	sampled: Option<f64>,
}

impl Timeline {
	/// Add a state that arrived at the time `arrival` on the clock of this machine.
	///
	/// `clock` is the time on the clock of the sender at which it made the state. A state that
	/// is not newer than the newest one is late, and the timeline drops it.
	pub fn push(&mut self, clock: u32, state: PlaneState, arrival: f64) {
		let sent = f64::from(clock);
		let travel = arrival - sent;
		match self.states.back() {
			Some(&(newest, _)) if sent <= newest => return,
			Some(_) => {}
			None => {
				self.offset = travel;
				self.lag = travel + tuning::MIN_DELAY + tuning::STATE_INTERVAL;
			}
		}
		self.travels.push_back((arrival, travel));
		while self
			.travels
			.front()
			.is_some_and(|&(at, _)| at < arrival - tuning::TRAVEL_WINDOW)
		{
			self.travels.pop_front();
		}
		self.offset = self
			.travels
			.iter()
			.map(|&(_, travel)| travel)
			.fold(f64::INFINITY, f64::min);
		self.jitter += ((travel - self.offset) - self.jitter) * tuning::JITTER_RESPONSE;
		self.states.push_back((sent, state));
		while self.states.len() > 2
			&& self
				.states
				.front()
				.is_some_and(|&(oldest, _)| oldest < sent - tuning::STATE_HISTORY)
		{
			self.states.pop_front();
		}
	}

	/// Return the delay behind the sender at which the plane shows now, beyond the travel time.
	#[must_use]
	pub fn delay(&self) -> f64 {
		self.lag - self.offset
	}

	/// Return the plane as it shows at the time `now` on the clock of this machine.
	///
	/// Call this once in each step, so that the delay follows the network at its slow, even
	/// pace. A timeline without a state shows nothing.
	pub fn sample(&mut self, now: f64) -> Option<PlaneState> {
		let elapsed = self.sampled.map_or(0.0, |last| (now - last).max(0.0));
		self.sampled = Some(now);
		let wanted = (tuning::STATE_INTERVAL + tuning::JITTER_COVER * self.jitter + tuning::DELAY_MARGIN)
			.clamp(tuning::MIN_DELAY, tuning::MAX_DELAY);
		let error = self.offset + wanted - self.lag;
		let pace = tuning::DELAY_PACE * elapsed;
		// Only a much shorter lag jumps, because it moves the plane forward. A longer lag would
		// replay motion, so it always grows slowly.
		self.lag += if error < -tuning::DELAY_SNAP {
			error
		} else {
			error.clamp(-pace, pace)
		};
		self.at(now - self.lag)
	}

	/// Return the plane at the time `shown` on the clock of the sender.
	fn at(&self, shown: f64) -> Option<PlaneState> {
		let &(oldest, first) = self.states.front()?;
		if shown <= oldest {
			return Some(first);
		}
		for index in 1..self.states.len() {
			let ((before, from), (after, to)) = (self.states[index - 1], self.states[index]);
			if shown <= after {
				return Some(blend(&from, &to, ((shown - before) / (after - before)) as f32));
			}
		}
		let &(newest, last) = self.states.back()?;
		let coast = (shown - newest).min(tuning::MAX_COAST) as f32 / 1000.0;
		Some(PlaneState {
			position: last.position + last.velocity() * coast * f32::from(u8::from(last.alive)),
			..last
		})
	}
}

/// Return the state at the part `t` of the way from `from` to `to`.
///
/// The place and the rotation move evenly. The other facts stay those of `from` until `to` is
/// reached. A plane that respawned is not drawn on the way between its two places.
fn blend(from: &PlaneState, to: &PlaneState, t: f32) -> PlaneState {
	if t >= 1.0 {
		return *to;
	}
	if !(from.alive && to.alive) || from.deaths != to.deaths {
		return *from;
	}
	PlaneState {
		position: from.position.lerp(to.position, t),
		orientation: from.orientation.slerp(to.orientation, t),
		speed: from.speed + (to.speed - from.speed) * t,
		..*from
	}
}

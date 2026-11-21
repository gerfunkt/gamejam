//! The timeline of a remote plane, fed through networks that deliver unevenly.

mod common;

use common::{Bus, toward};
use glam::{Quat, Vec3};
use sim::{
	Timeline, tuning,
	wire::{Message, PlaneState, Settings},
};

/// The difference between the clock of the sender and the clock of the receiver, in milliseconds.
/// The clocks of two machines never agree, and the timeline must not care.
const CLOCK_DIFFERENCE: f64 = 500_000.0;

/// The states that a machine sends for a plane that flies straight north for `seconds`, each with
/// the clock of the sender. `stall` names a time span in which the sender sends nothing.
fn flight(seconds: f32, stall: Option<(f64, f64)>) -> Vec<(u32, PlaneState)> {
	let mut bus = Bus::new(
		&[1],
		Settings {
			bots: 0,
			skill: sim::Skill::Rookie,
		},
	);
	bus.place(1, Vec3::new(0.0, 500.0, 0.0), Quat::IDENTITY);
	let start = bus.now;
	let mut states = Vec::new();
	for _ in 0..(seconds / tuning::STEP) as u32 {
		let now = bus.now;
		let machine = bus.machine(1);
		machine.step(now, &toward(Vec3::NEG_Z));
		let stalled = stall.is_some_and(|(from, to)| (from..to).contains(&(now - start)));
		for message in machine.drain_messages() {
			if let (Message::State { clock, state, .. }, false) = (message, stalled) {
				states.push((clock, state));
			}
		}
		bus.now += f64::from(tuning::STEP) * 1000.0;
	}
	states
}

/// What a receiver saw of a flight.
struct Seen {
	/// The farthest step backward along the heading, in metres. A smooth plane has none.
	back: f32,
	/// The longest time without any motion after the first half second, in milliseconds.
	frozen: f64,
	/// The delay at the end, in milliseconds.
	delay: f64,
	/// The largest delay in the last two seconds, in milliseconds.
	late_delay: f64,
}

/// Feed `states` to a timeline through a network, and watch the plane in each step.
///
/// `travel` gives the travel time of a state from its send time, both in milliseconds. The
/// network keeps the order of the states, as one TCP connection does.
fn watch(states: &[(u32, PlaneState)], travel: impl Fn(f64) -> f64) -> Seen {
	let first = f64::from(states[0].0);
	let last = f64::from(states[states.len() - 1].0);
	let mut arrivals = Vec::new();
	let mut latest = f64::NEG_INFINITY;
	for &(clock, state) in states {
		let sent = f64::from(clock);
		latest = latest.max(sent + travel(sent - first) - CLOCK_DIFFERENCE);
		arrivals.push((latest, clock, state));
	}
	let mut timeline = Timeline::default();
	let mut seen = Seen {
		back: 0.0,
		frozen: 0.0,
		delay: 0.0,
		late_delay: 0.0,
	};
	let (mut next, mut shown, mut still) = (0, None::<PlaneState>, 0.0);
	let step = f64::from(tuning::STEP) * 1000.0;
	let mut now = first - CLOCK_DIFFERENCE;
	while now < last - CLOCK_DIFFERENCE + 300.0 {
		while next < arrivals.len() && arrivals[next].0 <= now {
			let (arrival, clock, state) = arrivals[next];
			timeline.push(clock, state, arrival);
			next += 1;
		}
		if let Some(plane) = timeline.sample(now) {
			if let Some(before) = shown {
				let moved = (plane.position - before.position).dot(before.forward());
				seen.back = seen.back.max(-moved);
				still = if moved.abs() < 1e-3 { still + step } else { 0.0 };
				// The plane waits at its first state until the delay passed, and after its last one.
				if next < arrivals.len() && now > first - CLOCK_DIFFERENCE + 500.0 {
					seen.frozen = seen.frozen.max(still);
				}
			}
			shown = Some(plane);
			seen.delay = timeline.delay();
			if now > last - CLOCK_DIFFERENCE - 2000.0 && next < arrivals.len() {
				seen.late_delay = seen.late_delay.max(seen.delay);
			}
		}
		now += step;
	}
	assert_eq!(next, arrivals.len(), "the network did not deliver every state");
	assert!(shown.is_some(), "the timeline never showed the plane");
	seen
}

/// A repeatable sequence of numbers from 0 to 1.
fn noise(index: f64) -> f64 {
	let x = (index * 12.9898).sin() * 43_758.547;
	x - x.floor()
}

#[test]
fn a_plane_on_an_even_network_shows_at_the_shortest_delay_and_never_stops() {
	let seen = watch(&flight(8.0, None), |_| 1.0);
	assert_eq!(seen.back, 0.0);
	assert!(seen.frozen < 20.0, "the plane stood still for {} ms", seen.frozen);
	assert!((50.0..60.0).contains(&seen.delay), "the delay is {} ms", seen.delay);
}

#[test]
fn a_plane_whose_states_arrive_in_bunches_never_steps_backward() {
	// A VPN over TCP: 35 ms, and then a wait of up to 80 ms that several states share.
	let seen = watch(&flight(20.0, None), |sent| 35.0 + 80.0 * noise((sent / 120.0).floor()));
	assert_eq!(seen.back, 0.0, "the plane stepped {} m backward", seen.back);
	assert!(
		(80.0..300.0).contains(&seen.late_delay),
		"the delay settled at {} ms",
		seen.late_delay
	);
}

#[test]
fn a_plane_on_a_network_with_random_waits_never_steps_backward() {
	let seen = watch(&flight(20.0, None), |sent| 20.0 + 60.0 * noise(sent));
	assert_eq!(seen.back, 0.0, "the plane stepped {} m backward", seen.back);
	assert!(seen.late_delay < 300.0, "the delay settled at {} ms", seen.late_delay);
}

#[test]
fn the_delay_returns_after_a_stall_of_the_sender() {
	// The page of the sender stalls for one second. Its clock runs on, so the next state says so.
	let seen = watch(&flight(12.0, Some((3000.0, 4000.0))), |_| 5.0);
	assert_eq!(seen.back, 0.0, "the plane stepped {} m backward", seen.back);
	assert!(
		seen.late_delay < 70.0,
		"the delay is still {} ms, eight seconds after the stall",
		seen.late_delay
	);
}

#[test]
fn the_delay_returns_after_a_change_to_a_slower_route() {
	let seen = watch(&flight(14.0, None), |sent| if sent < 3000.0 { 5.0 } else { 90.0 });
	assert_eq!(seen.back, 0.0, "the plane stepped {} m backward", seen.back);
	assert!(
		seen.late_delay < 70.0,
		"the delay is still {} ms, long after the change",
		seen.late_delay
	);
}

#[test]
fn a_plane_without_a_newer_state_coasts_briefly_and_then_waits() {
	let states = flight(2.0, None);
	let mut timeline = Timeline::default();
	for &(clock, state) in &states {
		timeline.push(clock, state, f64::from(clock));
	}
	let (last_clock, last) = states[states.len() - 1];
	let newest = f64::from(last_clock);
	let soon = timeline.sample(newest + 200.0).expect("the timeline holds states");
	let coasted = (soon.position - last.position).dot(last.forward());
	assert!(coasted > 5.0, "the plane did not coast: {coasted} m");

	let later = timeline.sample(newest + 3000.0).expect("the timeline holds states");
	let far = (later.position - last.position).length();
	let most = last.speed * (tuning::MAX_COAST as f32) / 1000.0;
	assert!(far <= most + 0.1, "the plane coasted {far} m, and may coast {most} m");
}

#[test]
fn a_late_state_does_not_move_the_plane_back() {
	let states = flight(1.0, None);
	let mut timeline = Timeline::default();
	for &(clock, state) in &states {
		timeline.push(clock, state, f64::from(clock));
	}
	let now = f64::from(states[states.len() - 1].0) + 500.0;
	let before = timeline.sample(now).expect("the timeline holds states");
	let (old_clock, old) = states[2];
	timeline.push(
		old_clock,
		PlaneState {
			position: old.position + Vec3::X * 900.0,
			..old
		},
		now,
	);
	assert_eq!(timeline.sample(now), Some(before));
}

#[test]
fn an_empty_timeline_shows_nothing() {
	assert_eq!(Timeline::default().sample(1000.0), None);
}

//! The events of a match, as the sound hears them.
//!
//! Each frame, the sound reads the match and the plane of the player, and raises named events.
//! The conditions of the events are all here. The lists of sounds and the playing are in the
//! parent module, which decides what each event sounds like.

use bevy::prelude::*;
use sim::{
	Event, Match, Missile, Part, Plane, Session,
	arena::ground_height,
	tuning,
	wire::{PlaneId, PlaneState},
};

use super::perform;
use crate::flow::{Game, Screen};

/// The distance at which a sound in the sky is no longer heard, in metres.
const HEARING_RANGE: f32 = 1600.0;
/// The height above the ground under which Betty calls the altitude, in metres.
const LOW_HEIGHT: f32 = 80.0;
/// The height above the ground under which a dive needs a pull up, in metres.
const DIVE_HEIGHT: f32 = 200.0;
/// The angle below the horizon at which a nose points steeply down, in radians.
const STEEP_DIVE: f32 = 0.3;
/// The speed of descent that is too fast for the call of the altitude, in m/s.
const FAST_SINK: f32 = 20.0;
/// The turn of the nose from which Betty calls the load, in rad/s, and the speed it needs, in m/s.
const HARD_TURN: f32 = 1.0;
const HARD_TURN_SPEED: f32 = 80.0;
/// The part of the boost speed from which the plane flies at its maximum speed.
const MAX_SPEED_SHARE: f32 = 0.95;
/// The angle from the nose within which a plane is the target of the nose, in radians.
const NOSE_CONE: f32 = 0.2;
/// The health of a part, as a share of its full health, under which Betty cautions.
const CAUTION_SHARE: f32 = 0.5;
/// The health of the fuselage, as a share of its full health, under which Betty warns twice.
const CRITICAL_SHARE: f32 = 0.3;
/// What the sound does for the match.
pub(super) enum Cue {
	/// An event happened: play its sounds once, at this loudness.
	Raise(&'static str, f32),
	/// A state lasts: raise the event again when the pause of its lines is over.
	Repeat(&'static str),
	/// The loops of an event play at this loudness and this speed. A loudness of 0 stops them.
	Hold(&'static str, f32, f32),
	/// Stop every loop.
	Silence,
}

/// The cues of one frame, with the checks of the edge of a state.
#[derive(Default)]
pub(super) struct Cues(pub(super) Vec<Cue>);

impl Cues {
	fn raise(&mut self, event: &'static str, loudness: f32) {
		self.0.push(Cue::Raise(event, loudness));
	}

	/// Raise `event` in the frame in which `now` turns on.
	fn on(&mut self, event: &'static str, was: bool, now: bool) {
		if now && !was {
			self.raise(event, 1.0);
		}
	}

	fn repeat(&mut self, event: &'static str) {
		self.0.push(Cue::Repeat(event));
	}

	fn hold(&mut self, event: &'static str, loudness: f32, speed: f32) {
		self.0.push(Cue::Hold(event, loudness, speed));
	}
}

/// What the match shows in one frame, for the player of this machine.
struct Frame<'a> {
	events: &'a [Event],
	me: PlaneId,
	own: &'a Plane,
	planes: Vec<(PlaneId, PlaneState)>,
	missiles: &'a [Missile],
	missiles_ready: u8,
	lock: Option<(PlaneId, f32)>,
	alive: bool,
	firing: bool,
	dt: f32,
}

impl<'a> Frame<'a> {
	fn of(running: &'a Match, events: &'a [Event], dt: f32) -> Option<Self> {
		let me = running.me();
		let planes = running.planes();
		let firing = planes.iter().any(|(id, state)| *id == me && state.firing);
		Some(Self {
			events,
			me,
			own: running.plane(me)?,
			planes,
			missiles: running.missiles(),
			missiles_ready: running.missiles_ready(me).unwrap_or(0),
			lock: running.lock(me),
			alive: running.respawn_in(me).is_none(),
			firing,
			dt,
		})
	}

	fn place_of(&self, plane: PlaneId) -> Vec3 {
		self.planes
			.iter()
			.find(|(id, _)| *id == plane)
			.map_or(self.own.position, |(_, state)| state.position)
	}

	/// Return the other planes that fly.
	fn others(&self) -> impl Iterator<Item = &(PlaneId, PlaneState)> {
		self.planes.iter().filter(|(id, state)| *id != self.me && state.alive)
	}
}

/// What the sound remembers of the last frame, to hear what changed.
#[derive(Default)]
pub(super) struct Heard {
	in_match: bool,
	alive: bool,
	firing: bool,
	/// The time until the next shot of the gun, in seconds.
	shot: f32,
	boosting: bool,
	at_max_speed: bool,
	protected: bool,
	missiles: u8,
	lock: f32,
	enemy_lock: bool,
	target_protected: bool,
	/// The missiles that follow the player, each with its shooter and its number.
	threats: Vec<(PlaneId, u8)>,
	nose: Vec3,
}

/// Return how loud a sound at `place` is for a listener at `ear`, from 0 to 1.
fn nearness(place: Vec3, ear: Vec3) -> f32 {
	(1.0 - place.distance(ear) / HEARING_RANGE).max(0.0).powi(2)
}

/// Raise the events that the match reports in this frame.
fn happenings(frame: &Frame, cues: &mut Cues) {
	for event in frame.events {
		match event {
			Event::Hit { by, .. } if *by == frame.me => cues.raise("hit_landed", 0.35),
			Event::Damage { plane, part, broke } if *plane == frame.me => damage(frame, *part, *broke, cues),
			Event::Kill { victim, killer } => {
				let near = nearness(frame.place_of(*victim), frame.own.position);
				cues.raise("plane_killed", if *victim == frame.me { 1.0 } else { near });
				if *killer == Some(frame.me) {
					cues.raise("kill_scored", 1.0);
				} else if *victim == frame.me {
					cues.raise("player_down", 1.0);
				}
			}
			Event::Burst { position } => cues.raise("missile_burst", nearness(*position, frame.own.position)),
			_ => {}
		}
	}
}

/// Raise the events of damage to the plane of the player.
fn damage(frame: &Frame, part: Part, broke: bool, cues: &mut Cues) {
	cues.raise("hit_taken", 0.8);
	let share = frame.own.health(part) / part.full_health();
	if broke {
		match part {
			Part::LeftWing => cues.raise("part_broken_left_wing", 0.9),
			Part::RightWing => cues.raise("part_broken_right_wing", 0.9),
			Part::Tail => cues.raise("part_broken_tail", 0.9),
			Part::Engine => cues.raise("part_broken_engine", 0.9),
			// The kill that follows has its own events.
			Part::Fuselage => {}
		}
		return;
	}
	cues.raise("damage_survived", 1.0);
	if part == Part::Fuselage && share < CRITICAL_SHARE {
		cues.raise("fuselage_critical", 0.8);
	} else if share < CAUTION_SHARE {
		cues.raise("part_damaged_caution", 0.8);
	}
}

/// Raise the events of the gun, the missiles, and the seeker of the player.
fn weapons(frame: &Frame, heard: &mut Heard, cues: &mut Cues) {
	let firing = frame.alive && frame.firing;
	if firing {
		if !heard.firing {
			cues.raise("gun_start", 1.0);
			heard.shot = 0.0;
		}
		heard.shot -= frame.dt;
		if heard.shot <= 0.0 {
			cues.raise("gun_fire", 0.35);
			heard.shot += tuning::FIRE_INTERVAL;
		}
	}
	heard.firing = firing;
	if frame.missiles_ready < heard.missiles {
		cues.raise("missile_launched_by_me", 0.8);
	}
	heard.missiles = frame.missiles_ready;

	let closed = frame.lock.filter(|_| frame.alive).map_or(0.0, |(_, closed)| closed);
	cues.hold(
		"lock_tracking",
		if closed > 0.0 && closed < 1.0 { 0.5 } else { 0.0 },
		1.0,
	);
	if closed >= 1.0 && heard.lock < 1.0 {
		cues.raise("lock_acquired", 0.6);
	}
	heard.lock = closed;

	let aimed = frame.alive
		&& frame.others().any(|(id, state)| {
			let to = state.position - frame.own.position;
			state.protected
				&& (frame.lock.is_some_and(|(target, _)| target == *id)
					|| (to.length() <= tuning::LOCK_RANGE && frame.own.forward().angle_between(to) < NOSE_CONE))
		});
	cues.on("target_is_protected", heard.target_protected, aimed);
	heard.target_protected = aimed;
}

/// Raise the events of the planes and the missiles that threaten the player.
fn threats(frame: &Frame, heard: &mut Heard, cues: &mut Cues) {
	let enemy_lock = frame.alive && frame.others().any(|(_, state)| state.lock == Some(frame.me));
	cues.on("enemy_lock_on_me", heard.enemy_lock, enemy_lock);
	heard.enemy_lock = enemy_lock;

	let following: Vec<&Missile> = frame
		.missiles
		.iter()
		.filter(|missile| frame.alive && missile.target == frame.me && missile.is_seeking() && !missile.is_lured())
		.collect();
	let threats: Vec<(PlaneId, u8)> = following
		.iter()
		.map(|missile| (missile.shooter, missile.number))
		.collect();
	for _ in threats.iter().filter(|threat| !heard.threats.contains(threat)) {
		cues.raise("missile_incoming", 1.0);
	}
	for (shooter, number) in heard.threats.iter().filter(|threat| !threats.contains(threat)) {
		let lured = frame
			.missiles
			.iter()
			.any(|missile| (missile.shooter, missile.number) == (*shooter, *number) && missile.is_lured());
		if lured {
			cues.raise("missile_evaded", 0.8);
		}
	}
	heard.threats = threats;
}

/// Raise the events of the flight of the plane: the engine, the boost, the ground, and the load.
fn flight(frame: &Frame, heard: &mut Heard, cues: &mut Cues) {
	let (own, alive) = (frame.own, frame.alive);
	let pace = own.speed / tuning::BOOST_SPEED;
	let engine = alive && !own.is_broken(Part::Engine);
	cues.hold(
		"engine_loop",
		if engine { 0.12 + 0.2 * pace } else { 0.0 },
		0.7 + 0.7 * pace,
	);
	cues.hold(
		"wind_rush",
		if alive { 0.25 * pace * pace } else { 0.0 },
		0.8 + 0.4 * pace,
	);

	let boosting = alive && own.is_boosting();
	cues.on("boost_start", heard.boosting, boosting);
	cues.hold("boost_max_speed", if boosting { 0.3 } else { 0.0 }, 1.0);
	let at_max_speed = boosting && own.speed >= MAX_SPEED_SHARE * tuning::BOOST_SPEED;
	cues.on("boost_max_speed", heard.at_max_speed, at_max_speed);
	(heard.boosting, heard.at_max_speed) = (boosting, at_max_speed);

	let nose = own.forward();
	let height = own.position.y - ground_height(own.position.x, own.position.z);
	if alive && height < LOW_HEIGHT && own.velocity().y > -FAST_SINK {
		cues.repeat("low_altitude");
	}
	if alive && height < DIVE_HEIGHT && (-nose.y).asin() > STEEP_DIVE {
		cues.repeat("steep_dive_pullup");
	}
	let turn = if alive && heard.alive && frame.dt > 0.0 {
		heard.nose.angle_between(nose) / frame.dt
	} else {
		0.0
	};
	if turn > HARD_TURN && own.speed > HARD_TURN_SPEED {
		cues.repeat("hard_turn_over_g");
	}
	heard.nose = nose;

	let protected = alive && own.is_protected();
	cues.on("spawn_protected_start", heard.protected, protected);
	if heard.protected && !protected && alive {
		cues.raise("spawn_protected_end", 1.0);
	}
	heard.protected = protected;
}

/// Listen to one frame of the match, and return the cues for it.
fn listen(frame: &Frame, heard: &mut Heard) -> Cues {
	let mut cues = Cues::default();
	if !heard.in_match {
		cues.raise("match_start", 1.0);
	} else if frame.alive && !heard.alive {
		cues.raise("respawned", 1.0);
	}
	happenings(frame, &mut cues);
	weapons(frame, heard, &mut cues);
	threats(frame, heard, &mut cues);
	flight(frame, heard, &mut cues);
	(heard.in_match, heard.alive) = (true, frame.alive);
	cues
}

/// Play the sounds of the match: what happened in this frame, and what the plane of the player does.
pub(super) fn play_match(game: Res<Game>, time: Res<Time>, mut heard: Local<Heard>, mut before: Local<Option<Screen>>) {
	let mut cues = Cues::default();
	let in_match = matches!(game.screen, Screen::Playing | Screen::Paused);
	if matches!(*before, Some(Screen::Playing | Screen::Paused)) && game.screen == Screen::Results {
		cues.raise("match_end", 1.0);
	}
	*before = Some(game.screen);
	if !in_match {
		*heard = Heard::default();
	}
	let frame = game
		.session
		.as_ref()
		.and_then(Session::game)
		.filter(|_| game.screen == Screen::Playing)
		.and_then(|running| Frame::of(running, &game.events, time.delta_secs()));
	match frame {
		Some(frame) => cues.0.extend(listen(&frame, &mut heard).0),
		None => cues.0.push(Cue::Silence),
	}
	perform(cues, game.options.effects_volume, game.options.voice_volume);
}

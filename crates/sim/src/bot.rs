use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::{
	arena,
	missile::Missile,
	plane::Controls,
	powerup::PowerUp,
	tuning,
	wire::{PlaneId, PlaneState},
};

/// How well a bot flies and shoots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Skill {
	/// A beginner: wide turns, a loose aim, and no missiles.
	Rookie,
	/// An average pilot.
	Pilot,
	/// A good pilot.
	Veteran,
	/// The best pilot.
	Ace,
}

/// The numbers behind one skill.
struct Profile {
	/// How far ahead of its target the bot aims, as a part of the full lead. With a low part,
	/// the bullets pass behind a target that turns.
	lead: f32,
	/// The part of the full steering that the bot uses. With a low part, the bot turns wide.
	steering: f32,
	/// How far beyond its goal the bot holds its aim, as a factor of its aim error. With a low
	/// factor, the nose trails a target that turns.
	precision: f32,
	/// The throttle in a chase from far away.
	throttle: f32,
	/// The bot fires while its aim is inside this angle, in radians. A wide angle wastes bullets.
	fire_cone: f32,
	/// The farthest target at which the bot fires, in metres.
	fire_range: f32,
	/// Whether the bot slows down behind a slow target. A bot that does not, flies past it.
	matches_speed: bool,
	/// The farthest target at which the bot launches a missile, in metres. A rookie carries none.
	missile_range: Option<f32>,
	/// The distance of a missile at which the bot releases a flare, in metres. A late flare is
	/// too late, and a rookie releases none.
	flare_distance: f32,
}

impl Skill {
	/// Every skill, from the weakest to the strongest.
	pub const ALL: [Skill; 4] = [Skill::Rookie, Skill::Pilot, Skill::Veteran, Skill::Ace];

	/// Return the name of the skill, as the lobby shows it.
	#[must_use]
	pub fn name(self) -> &'static str {
		match self {
			Skill::Rookie => "Rookie",
			Skill::Pilot => "Pilot",
			Skill::Veteran => "Veteran",
			Skill::Ace => "Ace",
		}
	}

	/// Return the skill with this name, in any case of the letters.
	#[must_use]
	pub fn from_name(name: &str) -> Option<Self> {
		Self::ALL
			.into_iter()
			.find(|skill| skill.name().eq_ignore_ascii_case(name))
	}

	fn profile(self) -> Profile {
		match self {
			Skill::Rookie => Profile {
				lead: 0.6,
				steering: 0.6,
				precision: 2.5,
				throttle: 0.5,
				fire_cone: 0.10,
				fire_range: 320.0,
				matches_speed: false,
				missile_range: None,
				flare_distance: 0.0,
			},
			Skill::Pilot => Profile {
				lead: 0.8,
				steering: 0.8,
				precision: 2.5,
				throttle: 0.7,
				fire_cone: 0.08,
				fire_range: 420.0,
				matches_speed: true,
				missile_range: Some(550.0),
				flare_distance: 200.0,
			},
			Skill::Veteran => Profile {
				lead: 0.9,
				steering: 0.9,
				precision: 3.0,
				throttle: 0.85,
				fire_cone: 0.05,
				fire_range: 480.0,
				matches_speed: true,
				missile_range: Some(650.0),
				flare_distance: 300.0,
			},
			Skill::Ace => Profile {
				lead: 1.0,
				steering: 1.0,
				precision: 4.0,
				throttle: 0.95,
				fire_cone: 0.04,
				fire_range: 520.0,
				matches_speed: true,
				missile_range: Some(750.0),
				flare_distance: 400.0,
			},
		}
	}
}

/// Return the controls that a bot of the given `skill` chooses for the plane `me` in one step.
///
/// The bot chases a plane that can be hit, so it leaves a protected plane alone. It prefers a
/// near plane ahead of its nose. It aims ahead of its target, fires when its aim is good, and launches a missile when its lock
/// closed, and it releases a flare when a missile comes near. Without a target, it circles the
/// centre. The terrain comes before the fight.
///
/// A bot without a target, or with a target farther than [`tuning::BOT_POWERUP_TARGET_DISTANCE`],
/// flies for the nearest of `power_ups` that is there and within [`tuning::BOT_POWERUP_RANGE`].
#[must_use]
pub fn bot_controls(
	me: PlaneId,
	state: &PlaneState,
	planes: &[(PlaneId, PlaneState)],
	missiles: &[Missile],
	power_ups: &[PowerUp],
	skill: Skill,
) -> Controls {
	let profile = skill.profile();
	let forward = state.forward();
	// The bot prefers a plane ahead of its nose to a nearer plane behind it. Thus a bot that is
	// chased keeps its own chase, and two bots do not circle each other for ever.
	let rank = |other: &PlaneState| {
		let sight = other.position - state.position;
		sight.length() * (1.0 + tuning::BOT_ANGLE_WEIGHT * forward.angle_between(sight))
	};
	let target = planes
		.iter()
		.filter(|(id, other)| *id != me && other.can_be_hit())
		.map(|(_, other)| other)
		.min_by(|a, b| rank(a).total_cmp(&rank(b)));

	let pickup = power_ups
		.iter()
		.filter(|power_up| power_up.present)
		.map(|power_up| power_up.position)
		.filter(|&place| place.distance(state.position) <= tuning::BOT_POWERUP_RANGE)
		.min_by(|a, b| a.distance(state.position).total_cmp(&b.distance(state.position)))
		.filter(|_| {
			target.is_none_or(|target| target.position.distance(state.position) > tuning::BOT_POWERUP_TARGET_DISTANCE)
		});
	let target = target.filter(|_| pickup.is_none());

	let mut controls = Controls {
		aim: forward,
		throttle: profile.throttle,
		boost: false,
		fire: false,
		launch: false,
		flare: false,
	};
	let goal = match (target, pickup) {
		// The place where a bullet meets the target.
		(Some(target), _) => {
			let flight = target.position.distance(state.position) / tuning::BULLET_SPEED;
			target.position + target.velocity() * flight * profile.lead
		}
		(None, Some(place)) => place,
		// A place a little ahead on a ring around the centre.
		(None, None) => {
			let angle = state.position.z.atan2(state.position.x) + tuning::BOT_IDLE_LEAD;
			Vec3::new(angle.cos(), 0.0, angle.sin()) * tuning::BOT_IDLE_RADIUS + Vec3::Y * tuning::BOT_IDLE_HEIGHT
		}
	};
	let to_goal = goal - state.position;
	let distance = to_goal.length();
	let off_aim = forward.angle_between(to_goal);
	// The steering answers a small error with a small deflection, so the nose trails a goal that
	// moves. A good pilot holds the aim beyond the goal, and the nose then stays on it.
	let beyond = forward + (to_goal.normalize_or(forward) - forward) * profile.precision;
	controls.aim = limit_angle(forward, beyond, tuning::STEER_FULL_ANGLE * profile.steering);

	if let Some(target) = target {
		let ahead = off_aim < std::f32::consts::FRAC_PI_2;
		if profile.matches_speed && ahead && distance < tuning::BOT_CHASE_RANGE {
			// Close behind a target, a good pilot flies at its speed, and stays in firing range.
			let closing = ((distance - tuning::BOT_CHASE_DISTANCE) / tuning::BOT_CHASE_DISTANCE).clamp(-1.0, 1.0);
			let wanted = target.speed * (1.0 + 0.3 * closing);
			controls.throttle =
				((wanted - tuning::MIN_SPEED) / (tuning::MAX_SPEED - tuning::MIN_SPEED)).clamp(0.0, 1.0);
		}
		controls.fire = off_aim < profile.fire_cone && distance < profile.fire_range;
		controls.launch = profile.missile_range.is_some_and(|range| distance < range);
		if distance < tuning::BOT_EXTEND_RANGE && off_aim > tuning::BOT_EXTEND_ANGLE {
			// A near target beside or behind the bot cannot be shot. The bot does not circle
			// with it. It flies straight on at full throttle, and turns back from farther away.
			controls.aim = Vec3::new(forward.x, 0.0, forward.z).normalize_or(forward);
			controls.throttle = 1.0;
		}
	}

	// A missile that comes near gets a flare, if the pilot knows the trick.
	controls.flare = missiles.iter().any(|missile| {
		missile.target == me
			&& missile.is_seeking()
			&& !missile.is_lured()
			&& missile.position.distance(state.position) < profile.flare_distance
	});

	// The terrain comes before the fight. A dive needs height to end, the more the faster the
	// plane sinks, and a hill ahead counts like a hill below.
	let velocity = state.velocity();
	let ahead = state.position + velocity * tuning::BOT_PULL_UP_SECONDS;
	let floor = arena::clear_height(state.position.x, state.position.z).max(arena::clear_height(ahead.x, ahead.z))
		+ tuning::BOT_SAFE_HEIGHT;
	if state.position.y < floor + (-velocity.y).max(0.0) * tuning::BOT_PULL_UP_SECONDS {
		controls.aim = Vec3::new(forward.x, 0.0, forward.z).normalize_or(Vec3::NEG_Z) + Vec3::Y * 0.6;
	} else if state.position.y > tuning::CEILING - tuning::BOT_SAFE_HEIGHT {
		controls.aim = Vec3::new(controls.aim.x, controls.aim.y.min(-0.2), controls.aim.z);
	}
	controls
}

/// Return the direction that lies toward `wanted`, at most `limit` radians away from `from`.
fn limit_angle(from: Vec3, wanted: Vec3, limit: f32) -> Vec3 {
	let Some(wanted) = wanted.try_normalize() else {
		return from;
	};
	let angle = from.angle_between(wanted);
	if angle <= limit {
		wanted
	} else {
		from.slerp(wanted, limit / angle)
	}
}

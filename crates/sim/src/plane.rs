use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::{arena, steer::steer_toward, tuning};

/// A part of a plane that damage can break.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Part {
	/// The body. A plane without it is destroyed.
	Fuselage,
	/// The left wing. Without it, the plane spins to the left, falls, and is destroyed within seconds.
	LeftWing,
	/// The right wing. Without it, the plane spins to the right, falls, and is destroyed within seconds.
	RightWing,
	/// The tail. Without it, the plane loses most of its pitch and yaw control.
	Tail,
	/// The engine. Without it, the plane has no thrust.
	Engine,
}

impl Part {
	/// Every part, in the order of its index.
	pub const ALL: [Part; 5] = [
		Part::Fuselage,
		Part::LeftWing,
		Part::RightWing,
		Part::Tail,
		Part::Engine,
	];

	/// Return the health of this part on a new plane.
	#[must_use]
	pub fn full_health(self) -> f32 {
		match self {
			Part::Fuselage => tuning::FUSELAGE_HEALTH,
			Part::LeftWing | Part::RightWing => tuning::WING_HEALTH,
			Part::Tail => tuning::TAIL_HEALTH,
			Part::Engine => tuning::ENGINE_HEALTH,
		}
	}

	/// Return the centre of this part in the frame of the plane, in metres.
	#[must_use]
	pub fn centre(self) -> Vec3 {
		match self {
			Part::Fuselage => Vec3::ZERO,
			Part::LeftWing => Vec3::new(-3.1, 0.0, 0.3),
			Part::RightWing => Vec3::new(3.1, 0.0, 0.3),
			Part::Tail => Vec3::new(0.0, 0.5, 3.3),
			Part::Engine => Vec3::new(0.0, 0.0, -3.3),
		}
	}

	/// Return half of the width, the height, and the length of the box that shows this part, in metres.
	#[must_use]
	pub fn half_extents(self) -> Vec3 {
		match self {
			Part::Fuselage => Vec3::new(0.6, 0.6, 2.6),
			Part::LeftWing | Part::RightWing => Vec3::new(2.5, 0.12, 0.9),
			Part::Tail => Vec3::new(1.5, 0.7, 0.6),
			Part::Engine => Vec3::new(0.5, 0.5, 0.7),
		}
	}

	/// Return the radius of the sphere that a shot must touch to hit this part, in metres.
	///
	/// The sphere is a little larger than the box, which forgives a near miss.
	#[must_use]
	pub fn hit_radius(self) -> f32 {
		match self {
			Part::Fuselage => 1.6,
			Part::LeftWing | Part::RightWing => 2.1,
			Part::Tail => 1.5,
			Part::Engine => 1.3,
		}
	}
}

/// What a pilot asks of a plane in one step.
///
/// A player and a bot fly through the same controls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Controls {
	/// The direction in the world toward which the plane turns.
	pub aim: Vec3,
	/// The engine setting, from 0 for the slowest flight to 1 for the fastest.
	pub throttle: f32,
	/// Whether the pilot spends boost for more speed.
	pub boost: bool,
	/// Whether the pilot holds the trigger of the gun.
	pub fire: bool,
	/// Whether the pilot asks for a missile launch.
	pub launch: bool,
	/// Whether the pilot asks for a flare, which lures a missile away.
	pub flare: bool,
}

/// One plane: where it is, how it flies, and which of its parts still hold.
#[derive(Clone, Debug, PartialEq)]
pub struct Plane {
	/// The position of the centre of the fuselage in the world, in metres.
	pub position: Vec3,
	/// The rotation from the frame of the plane to the world.
	pub orientation: Quat,
	/// The speed along the nose, in m/s.
	pub speed: f32,
	health: [f32; Part::ALL.len()],
	/// The pitch, yaw, and roll rates, in rad/s.
	rates: Vec3,
	sink: f32,
	/// The seconds since the plane lost a wing.
	falling: f32,
	boost: f32,
	boosting: bool,
	/// The seconds that a speed power-up still lasts.
	speed_up: f32,
	protection: f32,
}

impl Plane {
	/// Return a new, whole plane in level cruise at `position`, with the given `orientation`.
	#[must_use]
	pub fn new(position: Vec3, orientation: Quat) -> Self {
		Self {
			position,
			orientation,
			speed: (tuning::MIN_SPEED + tuning::MAX_SPEED) / 2.0,
			health: Part::ALL.map(Part::full_health),
			rates: Vec3::ZERO,
			sink: 0.0,
			falling: 0.0,
			boost: tuning::BOOST_SECONDS,
			boosting: false,
			speed_up: 0.0,
			protection: 0.0,
		}
	}

	/// Return a plane in the middle of a flight, with the given speed and the health of each part.
	///
	/// A machine that takes a plane over from another machine continues it with this.
	#[must_use]
	pub fn restored(position: Vec3, orientation: Quat, speed: f32, health: [f32; Part::ALL.len()]) -> Self {
		Self {
			speed,
			health,
			..Self::new(position, orientation)
		}
	}

	/// Return the direction of the nose in the world.
	#[must_use]
	pub fn forward(&self) -> Vec3 {
		self.orientation * Vec3::NEG_Z
	}

	/// Return the velocity in the world, in m/s.
	#[must_use]
	pub fn velocity(&self) -> Vec3 {
		self.forward() * self.speed + Vec3::NEG_Y * self.sink
	}

	/// Return the centre of `part` in the world.
	#[must_use]
	pub fn part_position(&self, part: Part) -> Vec3 {
		self.position + self.orientation * part.centre()
	}

	/// Return the health that `part` has left. A broken part has none.
	#[must_use]
	pub fn health(&self, part: Part) -> f32 {
		self.health[part as usize]
	}

	/// Return whether `part` broke off.
	#[must_use]
	pub fn is_broken(&self, part: Part) -> bool {
		self.health(part) <= 0.0
	}

	/// Return whether the plane is out of the fight: its fuselage broke, or it hit the ground.
	#[must_use]
	pub fn is_destroyed(&self) -> bool {
		self.is_broken(Part::Fuselage)
	}

	/// Return the boost that is left, from 0 for an empty tank to 1 for a full one.
	#[must_use]
	pub fn boost(&self) -> f32 {
		self.boost / tuning::BOOST_SECONDS
	}

	/// Return whether the plane spends boost now.
	#[must_use]
	pub fn is_boosting(&self) -> bool {
		self.boosting
	}

	/// Return the seconds that a speed power-up still lasts, or 0 for a plane without one.
	#[must_use]
	pub fn speed_up_left(&self) -> f32 {
		self.speed_up
	}

	/// Start a speed power-up of `seconds`. While it lasts, the level speed is higher by
	/// [`tuning::POWERUP_SPEED_BONUS`], and boost costs nothing.
	pub fn grant_speed_up(&mut self, seconds: f32) {
		self.speed_up = seconds;
	}

	/// Protect the plane from all damage for `seconds`.
	pub fn protect(&mut self, seconds: f32) {
		self.protection = seconds;
	}

	/// Return whether the plane is still protected from damage after its spawn.
	#[must_use]
	pub fn is_protected(&self) -> bool {
		self.protection > 0.0
	}

	/// Take `points` of health from `part`, and return whether this broke the part.
	///
	/// A protected plane and a part that already broke take no damage.
	pub fn damage(&mut self, part: Part, points: f32) -> bool {
		if self.is_protected() || self.is_broken(part) {
			return false;
		}
		let health = &mut self.health[part as usize];
		*health = (*health - points).max(0.0);
		*health <= 0.0
	}

	/// Make every part whole again, as a repair power-up does.
	pub fn repair(&mut self) {
		self.health = Part::ALL.map(Part::full_health);
	}

	/// Destroy the plane at once, as a crash does.
	pub fn destroy(&mut self) {
		self.health[Part::Fuselage as usize] = 0.0;
	}

	/// Advance the flight by `dt` seconds under `controls`. A destroyed plane does not move.
	///
	/// A plane that touches the terrain is destroyed. A plane that lost a wing no longer answers
	/// its pilot: it falls, and it is destroyed within [`tuning::WING_LOSS_SECONDS`].
	pub fn step(&mut self, controls: &Controls, dt: f32) {
		if self.is_destroyed() {
			return;
		}
		self.protection = (self.protection - dt).max(0.0);
		self.speed_up = (self.speed_up - dt).max(0.0);
		if self.wings() < 2 {
			self.fall(dt);
		} else {
			self.turn(controls, dt);
			let lift = self.lift();
			self.drop_nose(tuning::NOSE_DROP_RATE * (1.0 - lift) * dt);
			self.sink = tuning::SINK_SPEED * (1.0 - lift);
			self.accelerate(controls, dt);
		}
		self.position += self.velocity() * dt;
		if self.falling >= tuning::WING_LOSS_SECONDS || arena::touches_terrain(self.position, tuning::PLANE_RADIUS) {
			self.destroy();
		}
	}

	/// Return whether the plane lost a wing and falls out of the sky.
	#[must_use]
	pub fn is_falling(&self) -> bool {
		!self.is_destroyed() && self.wings() < 2
	}

	/// Spin and drop a plane that lost a wing.
	fn fall(&mut self, dt: f32) {
		self.falling += dt;
		self.boosting = false;
		let lost_left = f32::from(self.is_broken(Part::LeftWing));
		let lost_right = f32::from(self.is_broken(Part::RightWing));
		// The plane spins toward the lost wing. Without both wings, it spins one way.
		let spin = if lost_left == lost_right {
			1.0
		} else {
			lost_right - lost_left
		};
		self.rates = Vec3::new(0.0, 0.0, spin * tuning::WRECK_SPIN);
		self.orientation = (self.orientation * Quat::from_rotation_z(-self.rates.z * dt)).normalize();
		self.drop_nose(tuning::WRECK_NOSE_DROP * dt);
		self.sink += tuning::WRECK_FALL * dt;
		self.speed = (self.speed - tuning::DRAG * dt).max(0.0);
	}

	/// Turn the nose toward the aim of the pilot, as far as the tail allows.
	fn turn(&mut self, controls: &Controls, dt: f32) {
		let steering = steer_toward(self.orientation, self.bounded_aim(controls.aim));
		let agility = agility(self.speed);
		let tail = if self.is_broken(Part::Tail) {
			tuning::NO_TAIL_CONTROL
		} else {
			1.0
		};
		let target = Vec3::new(
			steering.pitch * tuning::PITCH_RATE * agility * tail,
			steering.yaw * tuning::YAW_RATE * agility * tail,
			steering.roll * tuning::ROLL_RATE * agility,
		);
		self.rates += (target - self.rates) * (dt / tuning::RESPONSE_TIME).min(1.0);
		let turn = Quat::from_rotation_y(-self.rates.y * dt)
			* Quat::from_rotation_x(self.rates.x * dt)
			* Quat::from_rotation_z(-self.rates.z * dt);
		self.orientation = (self.orientation * turn).normalize();
	}

	/// Return `aim`, or the way back to the centre for a plane that left the arena.
	fn bounded_aim(&self, aim: Vec3) -> Vec3 {
		let from_centre = Vec3::new(self.position.x, 0.0, self.position.z);
		if from_centre.length() > tuning::ARENA_RADIUS {
			-from_centre
		} else {
			aim
		}
	}

	fn wings(&self) -> usize {
		[Part::LeftWing, Part::RightWing]
			.into_iter()
			.filter(|&wing| !self.is_broken(wing))
			.count()
	}

	/// Return the lift, from 0 for none to 1 for level flight without effort.
	fn lift(&self) -> f32 {
		let speed = smoothstep((self.speed - tuning::STALL_LOW) / (tuning::STALL_HIGH - tuning::STALL_LOW));
		let air = 1.0 - ((self.position.y - tuning::CEILING) / tuning::CEILING_FADE).clamp(0.0, 1.0);
		speed * air
	}

	/// Turn the nose toward the ground by `angle` radians.
	fn drop_nose(&mut self, angle: f32) {
		let axis = self.forward().cross(Vec3::NEG_Y);
		if let Some(axis) = axis.try_normalize() {
			self.orientation = (Quat::from_axis_angle(axis, angle) * self.orientation).normalize();
		}
	}

	/// Move the speed toward the speed that the engine, the boost, and the slope of the flight give.
	fn accelerate(&mut self, controls: &Controls, dt: f32) {
		let engine = !self.is_broken(Part::Engine);
		let sped_up = self.speed_up > 0.0;
		let boosting = controls.boost && engine && (sped_up || self.boost > 0.0);
		self.boosting = boosting;
		if boosting {
			if !sped_up {
				self.boost = (self.boost - dt).max(0.0);
			}
		} else if !controls.boost {
			// The tank fills only while the pilot does not ask for boost, so an empty tank stays empty.
			self.boost = (self.boost + tuning::BOOST_RECHARGE * dt).min(tuning::BOOST_SECONDS);
		}
		let level = if !engine {
			tuning::GLIDE_SPEED
		} else {
			let flight = if boosting {
				tuning::BOOST_SPEED
			} else {
				tuning::MIN_SPEED + (tuning::MAX_SPEED - tuning::MIN_SPEED) * controls.throttle.clamp(0.0, 1.0)
			};
			flight + if sped_up { tuning::POWERUP_SPEED_BONUS } else { 0.0 }
		};
		let target = (level - tuning::CLIMB_COST * self.forward().y).max(0.0);
		self.speed = if self.speed < target {
			let gain = if engine { tuning::THRUST } else { tuning::DIVE_GAIN };
			(self.speed + gain * dt).min(target)
		} else {
			(self.speed - tuning::DRAG * dt).max(target)
		};
	}
}

/// Return how well a plane turns at `speed`, from the line through the points of [`tuning::AGILITY`].
fn agility(speed: f32) -> f32 {
	let points = tuning::AGILITY;
	let (first_speed, first_value) = points[0];
	if speed <= first_speed {
		return first_value;
	}
	for pair in points.windows(2) {
		let ((low, from), (high, to)) = (pair[0], pair[1]);
		if speed <= high {
			return from + (to - from) * (speed - low) / (high - low);
		}
	}
	points[points.len() - 1].1
}

fn smoothstep(x: f32) -> f32 {
	let x = x.clamp(0.0, 1.0);
	x * x * (3.0 - 2.0 * x)
}

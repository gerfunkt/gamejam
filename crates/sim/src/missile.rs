//! Missiles: a lock on a plane ahead, a homing flight, and a burst that damages the parts near it.
//!
//! A missile is fast, but its seeker sees only a cone ahead, it turns slowly, and its fuel is
//! short. A target that breaks hard across its nose in time, or that outruns it from far behind,
//! escapes it.

use glam::Vec3;

use crate::{
	arena,
	plane::Part,
	tuning,
	wire::{PlaneId, PlaneState},
};

/// A missile in flight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Missile {
	/// The position in the world, in metres.
	pub position: Vec3,
	/// The direction of flight.
	pub direction: Vec3,
	/// The plane that launched the missile.
	pub shooter: PlaneId,
	/// The number of the missile among those of its shooter.
	pub number: u8,
	/// The plane that the missile follows.
	pub target: PlaneId,
	speed: f32,
	fuel: f32,
	/// A missile that lost sight of its target flies straight on, and never finds it again.
	seeking: bool,
	/// Whether the missile follows a flare in place of its target.
	lured: bool,
	/// Whether this machine decides the burst of the missile. The missile of a remote plane is only drawn.
	pub(crate) live: bool,
}

impl Missile {
	/// Return a missile that starts at `position` toward `direction`, at the speed of its shooter.
	pub(crate) fn new(
		shooter: PlaneId,
		number: u8,
		target: PlaneId,
		position: Vec3,
		direction: Vec3,
		live: bool,
	) -> Self {
		Self {
			position,
			direction: direction.normalize_or(Vec3::NEG_Z),
			shooter,
			number,
			target,
			speed: tuning::MISSILE_LAUNCH_SPEED,
			fuel: tuning::MISSILE_FUEL,
			seeking: true,
			lured: false,
			live,
		}
	}

	/// Return whether the fuel of the missile is gone.
	pub(crate) fn is_spent(&self) -> bool {
		self.fuel <= 0.0
	}

	/// Return whether the seeker still follows the target.
	#[must_use]
	pub fn is_seeking(&self) -> bool {
		self.seeking
	}

	/// Return whether the missile follows a flare in place of its target.
	#[must_use]
	pub fn is_lured(&self) -> bool {
		self.lured
	}

	/// Advance the missile by `dt` seconds, and return whether its fuse fires now.
	///
	/// The missile turns toward the place where its target will be. A flare of the target that
	/// its seeker sees takes the place of the target, unless the missile is already too near.
	/// The fuse fires when the path of this step passes near a plane that can be hit, apart from
	/// the shooter, when the missile reaches the flare that it follows, and when it touches the terrain.
	pub(crate) fn step(&mut self, planes: &[(PlaneId, PlaneState)], flares: &[Flare], dt: f32) -> bool {
		self.speed = (self.speed + tuning::MISSILE_THRUST * dt).min(tuning::MISSILE_SPEED);
		let target = planes
			.iter()
			.find(|(id, state)| *id == self.target && state.alive)
			.map(|(_, state)| state);
		let mut at_flare = false;
		if self.seeking {
			let sees = |place: Vec3| self.direction.angle_between(place - self.position) <= tuning::SEEKER_CONE;
			let in_time = self.lured
				|| target.is_some_and(|target| target.position.distance(self.position) > tuning::FLARE_MIN_DISTANCE);
			let flare = flares
				.iter()
				.filter(|flare| flare.owner == self.target && in_time && sees(flare.position))
				.min_by(|a, b| {
					a.position
						.distance(self.position)
						.total_cmp(&b.position.distance(self.position))
				});
			// What the seeker follows: a flare before the plane, with the place and the velocity of each.
			let followed = match (flare, target) {
				(Some(flare), _) => Some((flare.position, flare.velocity)),
				(None, Some(target)) if !self.lured && sees(target.position) => {
					Some((target.position, target.velocity()))
				}
				_ => None,
			};
			self.lured |= flare.is_some();
			at_flare = flare
				.is_some_and(|flare| flare.position.distance(self.position) <= tuning::MISSILE_FUSE + self.speed * dt);
			match followed {
				Some((place, velocity)) => {
					// The missile flies ahead of what it follows, but never so far ahead that
					// its own seeker loses it.
					let sight = place - self.position;
					let lead = sight + velocity * (sight.length() / self.speed);
					let most =
						(tuning::SEEKER_CONE * tuning::SEEKER_LEAD - self.direction.angle_between(sight)).max(0.0);
					let wanted = limit_angle(sight, lead, sight.angle_between(lead).min(most));
					self.turn_toward(wanted, tuning::MISSILE_TURN_RATE * dt);
				}
				None => self.seeking = false,
			}
		}
		let path = self.direction * self.speed * dt;
		let from = self.position;
		self.position += path;
		self.fuel -= dt;
		let near_plane = planes
			.iter()
			.filter(|(id, state)| *id != self.shooter && state.can_be_hit())
			.any(|(_, state)| {
				// The place on the path of this step that is nearest to the plane.
				let to_plane = state.position - from;
				let nearest = (to_plane.dot(path) / path.length_squared().max(1e-6)).clamp(0.0, 1.0);
				(to_plane - path * nearest).length() <= tuning::MISSILE_FUSE
			});
		near_plane || at_flare || arena::touches_terrain(self.position, 0.0)
	}

	fn turn_toward(&mut self, wanted: Vec3, limit: f32) {
		let Some(wanted) = wanted.try_normalize() else { return };
		let angle = self.direction.angle_between(wanted);
		if angle > 1e-4 {
			self.direction = self.direction.slerp(wanted, (limit / angle).min(1.0)).normalize();
		}
	}
}

/// Return the health that a burst at `position` takes from each part of `plane` in its reach.
///
/// The damage falls from [`tuning::BURST_DAMAGE`] at the centre of a part to
/// [`tuning::BURST_EDGE_DAMAGE`] at the edge of [`tuning::BURST_RADIUS`].
#[must_use]
pub fn burst_damage(position: Vec3, plane: &PlaneState) -> Vec<(Part, f32)> {
	Part::ALL
		.into_iter()
		.filter(|&part| !plane.is_broken(part))
		.filter_map(|part| {
			let centre = plane.position + plane.orientation * part.centre();
			let nearness = 1.0 - centre.distance(position) / tuning::BURST_RADIUS;
			let span = tuning::BURST_DAMAGE - tuning::BURST_EDGE_DAMAGE;
			(nearness >= 0.0).then_some((part, tuning::BURST_EDGE_DAMAGE + span * nearness))
		})
		.collect()
}

/// The missiles of one plane: the lock of the seeker, and the rack.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Launcher {
	target: Option<PlaneId>,
	held: f32,
	ready: u8,
	reload: f32,
	wait: f32,
	launched: u8,
}

impl Default for Launcher {
	fn default() -> Self {
		Self {
			target: None,
			held: 0.0,
			ready: tuning::MISSILES,
			reload: 0.0,
			wait: 0.0,
			launched: 0,
		}
	}
}

impl Launcher {
	/// Return the plane that the seeker follows, and how far the lock closed, from 0 to 1.
	pub(crate) fn lock(&self) -> Option<(PlaneId, f32)> {
		Some((self.target?, (self.held / tuning::LOCK_SECONDS).min(1.0)))
	}

	/// Return the number of missiles that are ready.
	pub(crate) fn ready(&self) -> u8 {
		self.ready
	}

	/// Fill the rack at once. The missiles that left keep their numbers.
	pub(crate) fn refill(&mut self) {
		(self.ready, self.reload) = (tuning::MISSILES, 0.0);
	}

	/// Advance the seeker and the rack of the plane `me` by `dt` seconds.
	///
	/// Return a missile when the pilot asks for a launch, the lock closed, and a missile is ready.
	pub(crate) fn step(
		&mut self,
		me: PlaneId,
		state: &PlaneState,
		planes: &[(PlaneId, PlaneState)],
		launch: bool,
		dt: f32,
	) -> Option<Missile> {
		self.wait = (self.wait - dt).max(0.0);
		if self.ready < tuning::MISSILES {
			self.reload += dt;
			if self.reload >= tuning::MISSILE_RELOAD {
				(self.ready, self.reload) = (self.ready + 1, 0.0);
			}
		}
		let seen = if state.alive {
			seen_target(me, state, planes)
		} else {
			None
		};
		// The lock starts again when the seeker sees another plane.
		self.held = if seen.is_some() && seen == self.target {
			self.held + dt
		} else {
			0.0
		};
		self.target = seen;
		let target = self.target?;
		if !launch || self.held < tuning::LOCK_SECONDS || self.ready == 0 || self.wait > 0.0 {
			return None;
		}
		self.ready -= 1;
		self.wait = tuning::MISSILE_INTERVAL;
		self.held = 0.0;
		self.launched = self.launched.wrapping_add(1);
		// The missile drops clear of the plane, below the nose, before it turns.
		let position = state.position + state.orientation * Vec3::new(0.0, -2.0, -6.0);
		Some(Missile::new(me, self.launched, target, position, state.forward(), true))
	}
}

/// Return the plane that the seeker of `me` sees: the one nearest to the line of the nose, inside
/// the lock cone and the lock range.
fn seen_target(me: PlaneId, state: &PlaneState, planes: &[(PlaneId, PlaneState)]) -> Option<PlaneId> {
	let forward = state.forward();
	planes
		.iter()
		.filter(|(id, other)| *id != me && other.can_be_hit())
		.filter_map(|(id, other)| {
			let sight = other.position - state.position;
			let angle = forward.angle_between(sight);
			(sight.length() <= tuning::LOCK_RANGE && angle <= tuning::LOCK_CONE).then_some((*id, angle))
		})
		.min_by(|a, b| a.1.total_cmp(&b.1))
		.map(|(id, _)| id)
}

/// Return the direction that lies toward `wanted`, at most `limit` radians away from `from`.
fn limit_angle(from: Vec3, wanted: Vec3, limit: f32) -> Vec3 {
	let angle = from.angle_between(wanted);
	if angle <= limit || angle < 1e-4 {
		wanted
	} else {
		from.normalize().slerp(wanted.normalize(), limit / angle)
	}
}

/// A flare that burns behind the plane that released it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flare {
	/// The position in the world, in metres.
	pub position: Vec3,
	/// The velocity in the world, in m/s.
	pub velocity: Vec3,
	/// The plane that released the flare.
	pub owner: PlaneId,
	life: f32,
}

impl Flare {
	pub(crate) fn new(owner: PlaneId, position: Vec3, velocity: Vec3) -> Self {
		Self {
			position,
			velocity,
			owner,
			life: tuning::FLARE_SECONDS,
		}
	}

	/// Move the flare by `dt` seconds: it slows down, and it falls.
	pub(crate) fn step(&mut self, dt: f32) {
		self.velocity *= (1.0 - tuning::FLARE_DRAG * dt).max(0.0);
		self.velocity.y -= tuning::FLARE_FALL * dt;
		self.position += self.velocity * dt;
		self.life -= dt;
	}

	pub(crate) fn is_spent(&self) -> bool {
		self.life <= 0.0 || arena::touches_terrain(self.position, 0.0)
	}
}

/// The flares of one plane.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FlareRack {
	ready: u8,
	reload: f32,
	wait: f32,
}

impl Default for FlareRack {
	fn default() -> Self {
		Self {
			ready: tuning::FLARES,
			reload: 0.0,
			wait: 0.0,
		}
	}
}

impl FlareRack {
	/// Return the number of flares that are ready.
	pub(crate) fn ready(&self) -> u8 {
		self.ready
	}

	/// Fill the rack at once.
	pub(crate) fn refill(&mut self) {
		(self.ready, self.reload) = (tuning::FLARES, 0.0);
	}

	/// Advance the rack by `dt` seconds, and return whether it releases a flare now.
	pub(crate) fn step(&mut self, release: bool, dt: f32) -> bool {
		self.wait = (self.wait - dt).max(0.0);
		if self.ready < tuning::FLARES {
			self.reload += dt;
			if self.reload >= tuning::FLARE_RELOAD {
				(self.ready, self.reload) = (self.ready + 1, 0.0);
			}
		}
		if !release || self.ready == 0 || self.wait > 0.0 {
			return false;
		}
		self.ready -= 1;
		self.wait = tuning::FLARE_INTERVAL;
		true
	}
}

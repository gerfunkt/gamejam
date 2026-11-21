use glam::Vec3;

use crate::{
	plane::Part,
	rng::Rng,
	tuning,
	wire::{PlaneId, PlaneState},
};

/// A bullet in flight. Each one is a tracer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bullet {
	/// The position in the world, in metres.
	pub position: Vec3,
	/// The velocity in the world, in m/s.
	pub velocity: Vec3,
	/// The plane that fired the bullet.
	pub shooter: PlaneId,
	life: f32,
	/// Whether this machine decides the hits of the bullet. The bullet of a remote plane is only drawn.
	pub(crate) live: bool,
}

impl Bullet {
	/// Move the bullet by `dt` seconds, and return the place where it was before.
	pub(crate) fn step(&mut self, dt: f32) -> Vec3 {
		let from = self.position;
		self.position += self.velocity * dt;
		self.life -= dt;
		from
	}

	pub(crate) fn is_spent(&self) -> bool {
		self.life <= 0.0 || self.position.y < 0.0
	}
}

/// The gun of one plane: it fires at a fixed rate while the pilot holds the trigger.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Gun {
	wait: f32,
}

impl Gun {
	/// Advance the gun by `dt` seconds, and return the bullet that it fires now, if any.
	pub(crate) fn step(
		&mut self,
		shooter: PlaneId,
		state: &PlaneState,
		live: bool,
		rng: &mut Rng,
		dt: f32,
	) -> Option<Bullet> {
		self.wait = (self.wait - dt).max(0.0);
		if !(state.firing && state.alive) || self.wait > 0.0 {
			return None;
		}
		self.wait = tuning::FIRE_INTERVAL;
		let forward = state.forward();
		let spread = tuning::BULLET_SPREAD;
		let scatter = state.orientation * Vec3::new(rng.range(-spread, spread), rng.range(-spread, spread), 0.0);
		Some(Bullet {
			position: state.position + forward * tuning::MUZZLE,
			velocity: (forward + scatter) * tuning::BULLET_SPEED + state.velocity(),
			shooter,
			life: tuning::BULLET_LIFE,
			live,
		})
	}
}

/// Return the part of a plane that a shot along the line from `from` to `to` hits first.
///
/// A part is hit when the line touches its hit sphere. A broken part is no longer there.
#[must_use]
pub fn hit_part(target: &PlaneState, from: Vec3, to: Vec3) -> Option<Part> {
	hit_part_along(target, from, to).map(|(part, _)| part)
}

/// Return the first hit and its nearest point as a fraction of the shot.
pub(crate) fn hit_part_along(target: &PlaneState, from: Vec3, to: Vec3) -> Option<(Part, f32)> {
	let to_local = target.orientation.inverse();
	let from = to_local * (from - target.position);
	let along = to_local * (to - target.position) - from;
	let length_squared = along.length_squared();
	Part::ALL
		.into_iter()
		.filter(|&part| !target.is_broken(part))
		.filter_map(|part| {
			// The place on the line that is nearest to the centre of the part, as a fraction of the line.
			let to_centre = part.centre() - from;
			let nearest = if length_squared > 0.0 {
				(to_centre.dot(along) / length_squared).clamp(0.0, 1.0)
			} else {
				0.0
			};
			let miss = (to_centre - along * nearest).length();
			(miss <= part.hit_radius()).then_some((part, nearest))
		})
		.min_by(|a, b| a.1.total_cmp(&b.1))
}

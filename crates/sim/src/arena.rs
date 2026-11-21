//! The arena: the ground with its hills, and the features that stand on it.
//!
//! Every machine builds the same arena from fixed numbers, so the rules and the picture agree.
//! A plane that touches the terrain is destroyed, and a shot ends there.

use std::sync::OnceLock;

use glam::{Quat, Vec3};

use crate::tuning;

/// A feature of the terrain: a block that stands on the ground.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Feature {
	/// The centre of the block in the world, in metres.
	pub centre: Vec3,
	/// Half of its width, its height, and its depth, in metres.
	pub half_extents: Vec3,
	/// Its turn about the vertical axis, in radians.
	pub turn: f32,
}

impl Feature {
	/// Return whether a ball of `radius` at `point` touches the block.
	fn touches(&self, point: Vec3, radius: f32) -> bool {
		let local = Quat::from_rotation_y(-self.turn) * (point - self.centre);
		local.abs().cmple(self.half_extents + radius).all()
	}
}

/// Return the height of the ground at a place, in metres. The hills are smooth and never negative.
#[must_use]
pub fn ground_height(x: f32, z: f32) -> f32 {
	let hills = (x / 420.0 + 1.3).sin() * (z / 510.0 - 0.7).cos();
	let ridges = (x / 190.0 + z / 260.0).sin();
	tuning::HILL_HEIGHT * (0.5 + 0.35 * hills + 0.15 * ridges)
}

/// Return a number from 0 to 1 that depends only on `seed`.
fn noise(seed: u32) -> f32 {
	let mut value = seed.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
	value ^= value >> 15;
	value = value.wrapping_mul(0x2C1B_3C6D);
	value ^= value >> 12;
	(value & 0xFFFF) as f32 / 65535.0
}

/// Return the features of the arena. Their places and sizes are fixed.
#[must_use]
pub fn features() -> &'static [Feature] {
	static FEATURES: OnceLock<Vec<Feature>> = OnceLock::new();
	FEATURES.get_or_init(|| {
		(0..tuning::FEATURES)
			.map(|index| {
				let seed = index * 7;
				let angle = noise(seed) * std::f32::consts::TAU;
				let distance = noise(seed + 1).sqrt() * (tuning::ARENA_RADIUS - 60.0);
				let (x, z) = (angle.cos() * distance, angle.sin() * distance);
				let height = 10.0 + noise(seed + 3).powi(2) * (tuning::FEATURE_MAX_HEIGHT - 10.0);
				let half_extents = Vec3::new(7.0 + noise(seed + 2) * 15.0, height / 2.0, 7.0 + noise(seed + 4) * 15.0);
				// A block stands a little in the ground, so that no gap shows on a slope.
				let foot = ground_height(x, z) - tuning::FEATURE_SUNK;
				Feature {
					centre: Vec3::new(x, foot + half_extents.y, z),
					half_extents,
					turn: angle * 3.0,
				}
			})
			.collect()
	})
}

/// Return whether a ball of `radius` at `point` touches the ground or a feature.
#[must_use]
pub fn touches_terrain(point: Vec3, radius: f32) -> bool {
	if point.y - radius < ground_height(point.x, point.z) {
		return true;
	}
	// No feature reaches above the highest hill with the tallest block on it.
	point.y - radius <= tuning::HILL_HEIGHT + tuning::FEATURE_MAX_HEIGHT
		&& features().iter().any(|feature| feature.touches(point, radius))
}

/// Return the height above which no terrain can be at a place: the ground with the tallest feature on it.
#[must_use]
pub fn clear_height(x: f32, z: f32) -> f32 {
	ground_height(x, z) + tuning::FEATURE_MAX_HEIGHT
}

//! The arena: the ground, as baked from the terrain model.
//!
//! Every machine reads the same height grid, so the rules and the picture agree. A plane that touches
//! the terrain is destroyed, and a shot ends there. The features of the terrain, such as the mountains
//! and the crater, are part of the grid.

use std::sync::LazyLock;

use glam::Vec3;

use crate::tuning;

mod grid;

pub use grid::{GridError, HeightGrid};

/// The ground of the arena, from `cargo xtask terrain`. The bytes are checked by a test of this module.
static GROUND: LazyLock<HeightGrid> = LazyLock::new(|| {
	HeightGrid::from_bytes(include_bytes!("arena/heights.bin")).expect("the baked height grid is valid")
});

/// Return the height grid of the ground, for a client that draws the terrain from its points.
#[must_use]
pub fn ground() -> &'static HeightGrid {
	&GROUND
}

/// Return the height of the ground at a place, in metres.
///
/// The height blends evenly between the grid points. Outside the square of [`tuning::TERRAIN_SIDE`]
/// metres, it is the height at the nearest place on the edge.
#[must_use]
pub fn ground_height(x: f32, z: f32) -> f32 {
	GROUND.height(x, z)
}

/// Return whether a ball of `radius` at `point` touches the terrain.
#[must_use]
pub fn touches_terrain(point: Vec3, radius: f32) -> bool {
	point.y - radius < ground_height(point.x, point.z)
}

/// Return the height above which no ground lies near a place, in metres.
///
/// "Near" is within [`tuning::TERRAIN_CLEAR_RADIUS`] metres in both horizontal directions. A slope beside
/// the place can thus rise above the ground under it, and a flyer that stays above this height clears the
/// slope. A bot uses it to stay above the terrain below and ahead of it. A power-up and a spawn use
/// it to float in open air.
#[must_use]
pub fn clear_height(x: f32, z: f32) -> f32 {
	GROUND.highest_near(x, z, tuning::TERRAIN_CLEAR_RADIUS)
}

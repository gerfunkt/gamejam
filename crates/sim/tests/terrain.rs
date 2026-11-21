//! The terrain of the arena: the baked ground, and every height that must clear it.

use glam::Vec3;
use sim::{arena, tuning};

#[test]
fn the_baked_terrain_starts_at_height_zero_and_rises_no_more_than_the_limit() {
	let ground = arena::ground();
	assert_eq!(ground.low(), 0.0, "the lowest point is at height 0");
	assert!(
		ground.high() <= tuning::TERRAIN_MAX_RISE && ground.high() > 0.9 * tuning::TERRAIN_MAX_RISE,
		"the highest point is {} m",
		ground.high()
	);
}

#[test]
fn the_ground_at_a_grid_point_is_the_height_of_that_point() {
	let ground = arena::ground();
	let place = |index: usize| (index as f32 / (ground.side() - 1) as f32 - 0.5) * tuning::TERRAIN_SIDE;
	for (column, row) in [(0, 0), (10, 200), (128, 128), (256, 256), (77, 3)] {
		let height = arena::ground_height(place(column), place(row));
		assert!(
			(height - ground.point(column, row)).abs() < 0.01,
			"the point {column} {row} is {} m, and the ground there is {height} m",
			ground.point(column, row)
		);
	}
}

#[test]
fn outside_the_square_the_ground_has_the_height_of_the_edge() {
	let edge = tuning::TERRAIN_SIDE / 2.0;
	assert_eq!(
		arena::ground_height(edge + 900.0, 100.0),
		arena::ground_height(edge, 100.0),
		"east of the square"
	);
	assert_eq!(
		arena::ground_height(-5.0e4, -5.0e4),
		arena::ground_height(-edge, -edge),
		"far past a corner"
	);
}

#[test]
fn a_ball_touches_the_terrain_when_it_dips_below_the_ground_by_its_radius() {
	let ground = arena::ground_height(300.0, -200.0);
	assert!(
		!arena::touches_terrain(Vec3::new(300.0, ground + 5.0, -200.0), 4.0),
		"above"
	);
	assert!(
		arena::touches_terrain(Vec3::new(300.0, ground + 3.0, -200.0), 4.0),
		"the radius reaches into the ground"
	);
	assert!(
		arena::touches_terrain(Vec3::new(300.0, ground - 1.0, -200.0), 0.0),
		"a point under the ground"
	);
}

#[test]
fn no_ground_within_the_clear_radius_of_a_place_reaches_above_its_clear_height() {
	let radius = tuning::TERRAIN_CLEAR_RADIUS;
	for index in 0..400 {
		let (x, z) = (
			(index * 37 % 101) as f32 * 45.0 - 2250.0,
			(index * 53 % 97) as f32 * 47.0 - 2250.0,
		);
		let clear = arena::clear_height(x, z);
		for (dx, dz) in [
			(0.0, 0.0),
			(radius, 0.0),
			(-radius, radius),
			(radius, radius),
			(0.0, -radius),
			(-radius / 2.0, radius / 3.0),
		] {
			let ground = arena::ground_height(x + dx, z + dz);
			assert!(
				ground <= clear + 0.01,
				"the ground at {x} {z} offset {dx} {dz} is {ground} m, above {clear} m"
			);
		}
	}
}

#[test]
fn the_clear_height_sees_a_mountain_beside_the_place_that_the_ground_below_does_not_show() {
	let mut found = false;
	for index in 0..2000 {
		let (x, z) = (
			(index * 37 % 101) as f32 * 45.0 - 2250.0,
			(index * 53 % 97) as f32 * 47.0 - 2250.0,
		);
		found |= arena::clear_height(x, z) > arena::ground_height(x, z) + 50.0;
	}
	assert!(
		found,
		"no place has a slope of 50 m beside it, so the radius is of no use"
	);
}

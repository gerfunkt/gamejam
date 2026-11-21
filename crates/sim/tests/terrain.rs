//! The terrain of the arena: the baked ground, and every height that must clear it.

use glam::Vec3;
use sim::{Match, Skill, arena, tuning, wire::Settings};

const NO_BOTS: Settings = Settings {
	bots: 0,
	skill: Skill::Rookie,
};

/// The position of the plane that a new match gives to the player of `seed`.
fn spawn(seed: u32) -> Vec3 {
	let game = Match::new(1, seed, NO_BOTS);
	game.plane(game.me()).expect("the machine flies its player").position
}

/// The highest ground on the straight line from `from` to `to`, in metres.
fn highest_ground_between(from: Vec3, to: Vec3) -> f32 {
	(0..=200)
		.map(|step| from.lerp(to, step as f32 / 200.0))
		.map(|point| arena::ground_height(point.x, point.z))
		.fold(f32::MIN, f32::max)
}

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

#[test]
fn the_air_between_the_highest_ground_and_the_ceiling_holds_the_heights_of_a_fight() {
	let highest_spawn = tuning::TERRAIN_MAX_RISE + tuning::SPAWN_MAX_CLEARANCE;
	assert!(
		highest_spawn + tuning::BOT_SAFE_HEIGHT < tuning::CEILING,
		"a spawn at {highest_spawn} m is too near to the ceiling"
	);
}

#[test]
fn a_plane_spawns_high_above_the_ground_around_it_for_any_seed() {
	for seed in 0..3000 {
		let position = spawn(seed);
		let clear = arena::clear_height(position.x, position.z);
		assert!(
			position.y >= clear + tuning::SPAWN_MIN_CLEARANCE - 0.01
				&& position.y <= clear + tuning::SPAWN_MAX_CLEARANCE + 0.01,
			"seed {seed} spawns at {} m over ground that clears at {clear} m",
			position.y
		);
		assert!(
			position.y < tuning::CEILING,
			"seed {seed} spawns at {} m, above the ceiling",
			position.y
		);
	}
}

#[test]
fn a_plane_that_flies_level_from_its_spawn_to_the_centre_clears_the_ground_on_its_way() {
	for seed in 0..3000 {
		let position = spawn(seed);
		let ground = highest_ground_between(position, Vec3::new(0.0, position.y, 0.0));
		assert!(
			position.y > ground + tuning::BOT_SAFE_HEIGHT,
			"seed {seed} spawns at {} m, and the ground on its way reaches {ground} m",
			position.y
		);
	}
}

#[test]
fn a_spawn_is_never_inside_the_ground_for_places_on_the_whole_spawn_ring() {
	for angle in 0..720 {
		for radius in [tuning::SPAWN_MIN_RADIUS, 700.0, tuning::SPAWN_MAX_RADIUS] {
			let angle = (angle as f32 * 0.5).to_radians();
			let (x, z) = (radius * angle.sin(), radius * angle.cos());
			let lowest = arena::clear_height(x, z) + tuning::SPAWN_MIN_CLEARANCE;
			let ground = highest_ground_between(Vec3::new(x, lowest, z), Vec3::new(0.0, lowest, 0.0));
			assert!(
				lowest > ground + tuning::BOT_SAFE_HEIGHT,
				"a lowest spawn at {x} {z} is {lowest} m, and the ground on its way reaches {ground} m"
			);
		}
	}
}

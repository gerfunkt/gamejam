//! The flight of one plane, through the public interface of the rules.

use std::f32::consts::FRAC_PI_2;

use glam::{Quat, Vec2, Vec3};
use sim::{Controls, FixedSteps, Part, Plane, aim_direction, level_frame, tuning};

/// A whole plane at 500 m that flies north, which is -Z.
fn plane() -> Plane {
	Plane::new(Vec3::new(0.0, 500.0, 0.0), Quat::IDENTITY)
}

fn controls(aim: Vec3, throttle: f32) -> Controls {
	Controls {
		aim,
		throttle,
		boost: false,
		fire: false,
		launch: false,
	}
}

fn fly(plane: &mut Plane, controls: Controls, seconds: f32) {
	for _ in 0..(seconds / tuning::STEP).round() as u32 {
		plane.step(&controls, tuning::STEP);
	}
}

/// The angle between the nose of `plane` and `direction`, in radians.
fn off_nose(plane: &Plane, direction: Vec3) -> f32 {
	plane.forward().angle_between(direction)
}

#[test]
fn level_flight_keeps_its_height_and_reaches_the_speed_of_the_throttle() {
	let mut plane = plane();
	fly(&mut plane, controls(Vec3::NEG_Z, 1.0), 10.0);
	assert!((plane.position.y - 500.0).abs() < 1.0, "height {}", plane.position.y);
	assert!((plane.speed - tuning::MAX_SPEED).abs() < 0.5, "speed {}", plane.speed);

	fly(&mut plane, controls(Vec3::NEG_Z, 0.0), 10.0);
	assert!((plane.speed - tuning::MIN_SPEED).abs() < 0.5, "speed {}", plane.speed);
}

#[test]
fn a_cursor_at_the_centre_aims_along_the_nose() {
	let banked = Quat::from_rotation_y(0.7) * Quat::from_rotation_z(1.1);
	let aim = aim_direction(banked, Vec2::ZERO);
	assert!(aim.angle_between(banked * Vec3::NEG_Z) < 1e-4);
}

#[test]
fn a_cursor_right_of_the_centre_aims_to_the_right_whatever_the_bank() {
	let level = aim_direction(Quat::IDENTITY, Vec2::new(0.5, 0.0));
	let banked = aim_direction(Quat::from_rotation_z(1.2), Vec2::new(0.5, 0.0));
	assert!(level.x > 0.4 && level.y.abs() < 1e-4, "level aim {level}");
	assert!(
		level.angle_between(banked) < 1e-4,
		"a bank moved the aim from {level} to {banked}"
	);
}

#[test]
fn the_level_frame_keeps_the_nose_and_levels_the_wings() {
	let banked = Quat::from_rotation_y(-0.4) * Quat::from_rotation_x(0.3) * Quat::from_rotation_z(0.9);
	let frame = level_frame(banked);
	assert!((frame * Vec3::NEG_Z).angle_between(banked * Vec3::NEG_Z) < 1e-4);
	assert!((frame * Vec3::X).y.abs() < 1e-4);
	assert!((frame * Vec3::Y).y > 0.0);
}

#[test]
fn the_plane_turns_toward_an_aim_at_its_side() {
	let mut plane = plane();
	fly(&mut plane, controls(Vec3::X, 0.5), 4.0);
	assert!(
		off_nose(&plane, Vec3::X) < 0.05,
		"still {} rad off the aim",
		off_nose(&plane, Vec3::X)
	);
	assert!(
		plane.position.x > 50.0,
		"the plane did not move to the right: {}",
		plane.position
	);
}

#[test]
fn the_plane_turns_toward_an_aim_behind_it() {
	let mut plane = plane();
	fly(&mut plane, controls(Vec3::Z, 0.5), 6.0);
	assert!(
		off_nose(&plane, Vec3::Z) < 0.05,
		"still {} rad off the aim",
		off_nose(&plane, Vec3::Z)
	);
}

#[test]
fn the_plane_levels_its_wings_after_a_turn() {
	let mut plane = plane();
	fly(&mut plane, controls(Vec3::X, 0.5), 8.0);
	let right = plane.orientation * Vec3::X;
	assert!(right.y.abs() < 0.02, "the wings stay banked: right wing at {}", right.y);
}

#[test]
fn a_climb_costs_speed_and_a_dive_gives_it() {
	let mut climber = plane();
	fly(&mut climber, controls(Vec3::new(0.0, 1.0, -1.0), 1.0), 8.0);
	assert!(climber.position.y > 700.0, "height {}", climber.position.y);
	assert!(climber.speed < tuning::MAX_SPEED - 15.0, "speed {}", climber.speed);

	let mut diver = Plane::new(Vec3::new(0.0, 1400.0, 0.0), Quat::IDENTITY);
	fly(&mut diver, controls(Vec3::new(0.0, -1.0, -1.0), 1.0), 8.0);
	assert!(diver.speed > tuning::MAX_SPEED + 15.0, "speed {}", diver.speed);
}

#[test]
fn a_slow_plane_turns_tighter_than_a_fast_one() {
	let mut slow = plane();
	let mut fast = plane();
	fly(&mut slow, controls(Vec3::NEG_Z, 0.0), 6.0);
	fly(
		&mut fast,
		Controls {
			boost: true,
			..controls(Vec3::NEG_Z, 1.0)
		},
		3.0,
	);
	fly(&mut slow, controls(Vec3::X, 0.0), 1.0);
	fly(
		&mut fast,
		Controls {
			boost: true,
			..controls(Vec3::X, 1.0)
		},
		1.0,
	);
	assert!(
		off_nose(&slow, Vec3::X) + 0.3 < off_nose(&fast, Vec3::X),
		"slow is {} rad off, fast is {} rad off",
		off_nose(&slow, Vec3::X),
		off_nose(&fast, Vec3::X)
	);
}

#[test]
fn boost_gives_speed_until_the_tank_is_empty() {
	let mut plane = plane();
	let boost = Controls {
		boost: true,
		..controls(Vec3::NEG_Z, 1.0)
	};
	fly(&mut plane, boost, 4.0);
	assert!(
		plane.speed > tuning::MAX_SPEED + 30.0,
		"speed under boost {}",
		plane.speed
	);

	fly(&mut plane, boost, tuning::BOOST_SECONDS + 4.0);
	assert_eq!(plane.boost(), 0.0, "the tank still holds boost");
	assert!(
		plane.speed < tuning::MAX_SPEED + 0.5,
		"speed with an empty tank {}",
		plane.speed
	);

	fly(&mut plane, controls(Vec3::NEG_Z, 1.0), 5.0);
	assert!(plane.boost() > 0.2, "the tank did not fill again: {}", plane.boost());
}

#[test]
fn a_lost_wing_rolls_the_plane_to_its_side_and_costs_height() {
	let mut whole = plane();
	let mut damaged = plane();
	damaged.damage(Part::LeftWing, tuning::WING_HEALTH);
	assert!(damaged.is_broken(Part::LeftWing));

	fly(&mut whole, controls(Vec3::NEG_Z, 1.0), 4.0);
	fly(&mut damaged, controls(Vec3::NEG_Z, 1.0), 4.0);

	let right = damaged.orientation * Vec3::X;
	assert!(
		right.y > 0.1,
		"the left side does not hang low: right wing at {}",
		right.y
	);
	assert!(
		damaged.position.y < whole.position.y - 20.0,
		"height {} against {}",
		damaged.position.y,
		whole.position.y
	);
	assert!(
		damaged.position.x < whole.position.x - 1.0,
		"the plane did not pull to the left: {}",
		damaged.position.x
	);
}

#[test]
fn a_plane_with_one_wing_turns_slowly_away_from_the_lost_side() {
	let mut toward = plane();
	let mut away = plane();
	for plane in [&mut toward, &mut away] {
		plane.damage(Part::LeftWing, tuning::WING_HEALTH);
	}
	fly(&mut toward, controls(Vec3::NEG_X, 0.5), 2.0);
	fly(&mut away, controls(Vec3::X, 0.5), 2.0);
	assert!(
		off_nose(&toward, Vec3::NEG_X) + 0.3 < off_nose(&away, Vec3::X),
		"toward the lost side {} rad off, away from it {} rad off",
		off_nose(&toward, Vec3::NEG_X),
		off_nose(&away, Vec3::X)
	);
}

#[test]
fn a_plane_with_one_wing_can_still_hold_its_height_with_the_nose_up() {
	let mut damaged = plane();
	damaged.damage(Part::RightWing, tuning::WING_HEALTH);
	fly(&mut damaged, controls(Vec3::new(0.0, 0.35, -1.0), 1.0), 10.0);
	assert!(damaged.position.y > 480.0, "height {}", damaged.position.y);
}

#[test]
fn a_lost_tail_removes_most_of_the_control() {
	let mut whole = plane();
	let mut damaged = plane();
	damaged.damage(Part::Tail, tuning::TAIL_HEALTH);
	fly(&mut whole, controls(Vec3::X, 0.5), 1.2);
	fly(&mut damaged, controls(Vec3::X, 0.5), 1.2);
	let turned = |plane: &Plane| plane.forward().angle_between(Vec3::NEG_Z);
	assert!(
		turned(&damaged) < 0.5 * turned(&whole),
		"turned {} rad against {} rad",
		turned(&damaged),
		turned(&whole)
	);
}

#[test]
fn a_dead_engine_removes_the_thrust() {
	let mut plane = plane();
	plane.damage(Part::Engine, tuning::ENGINE_HEALTH);
	fly(&mut plane, controls(Vec3::NEG_Z, 1.0), 8.0);
	assert!(plane.speed < tuning::STALL_HIGH, "speed {}", plane.speed);
	assert!(plane.position.y < 450.0, "height {}", plane.position.y);
}

#[test]
fn a_dive_keeps_a_plane_with_a_dead_engine_above_the_stall() {
	let mut plane = plane();
	plane.damage(Part::Engine, tuning::ENGINE_HEALTH);
	fly(&mut plane, controls(Vec3::new(0.0, -0.45, -1.0), 1.0), 8.0);
	assert!(plane.speed > tuning::STALL_HIGH, "speed {}", plane.speed);
	assert!(!plane.is_destroyed());
}

#[test]
fn a_plane_that_meets_the_ground_is_destroyed_and_stops() {
	let mut plane = Plane::new(Vec3::new(0.0, 60.0, 0.0), Quat::IDENTITY);
	fly(&mut plane, controls(Vec3::new(0.0, -1.0, -1.0), 1.0), 5.0);
	assert!(plane.is_destroyed());
	let wreck = plane.position;
	fly(&mut plane, controls(Vec3::NEG_Z, 1.0), 1.0);
	assert_eq!(plane.position, wreck, "the wreck moved");
}

#[test]
fn the_air_above_the_ceiling_does_not_carry_a_plane() {
	let mut plane = Plane::new(Vec3::new(0.0, tuning::CEILING + 400.0, 0.0), Quat::IDENTITY);
	fly(&mut plane, controls(Vec3::NEG_Z, 1.0), 6.0);
	assert!(
		plane.position.y < tuning::CEILING + 300.0,
		"height {}",
		plane.position.y
	);
}

#[test]
fn a_pilot_cannot_fly_far_out_of_the_arena() {
	let mut plane = Plane::new(
		Vec3::new(tuning::ARENA_RADIUS - 200.0, 500.0, 0.0),
		Quat::from_rotation_y(-FRAC_PI_2),
	);
	assert!(plane.forward().x > 0.99, "the test plane must fly outward");
	let mut farthest: f32 = 0.0;
	for _ in 0..(40.0 / tuning::STEP) as u32 {
		plane.step(&controls(Vec3::X, 1.0), tuning::STEP);
		farthest = farthest.max(plane.position.x);
	}
	assert!(farthest > tuning::ARENA_RADIUS, "the plane never reached the edge");
	assert!(
		farthest < tuning::ARENA_RADIUS + 300.0,
		"the plane flew out to {farthest} m"
	);
	assert!(!plane.is_destroyed());
}

#[test]
fn damage_breaks_a_part_when_its_health_is_gone() {
	let mut plane = plane();
	assert!(
		!plane.damage(Part::Tail, tuning::TAIL_HEALTH - 1.0),
		"the tail broke early"
	);
	assert!(!plane.is_broken(Part::Tail));
	assert!(plane.damage(Part::Tail, 5.0), "the tail did not break");
	assert_eq!(plane.health(Part::Tail), 0.0);
	assert!(!plane.damage(Part::Tail, 5.0), "a broken part broke again");
	assert!(!plane.is_destroyed(), "a lost tail destroyed the plane");

	plane.damage(Part::Fuselage, tuning::FUSELAGE_HEALTH);
	assert!(plane.is_destroyed());
}

#[test]
fn a_protected_plane_takes_no_damage_until_the_protection_ends() {
	let mut plane = plane();
	plane.protect(5.0);
	plane.damage(Part::LeftWing, 30.0);
	assert_eq!(plane.health(Part::LeftWing), tuning::WING_HEALTH);

	fly(&mut plane, controls(Vec3::NEG_Z, 1.0), 5.1);
	assert!(!plane.is_protected());
	plane.damage(Part::LeftWing, 30.0);
	assert_eq!(plane.health(Part::LeftWing), tuning::WING_HEALTH - 30.0);
}

#[test]
fn the_rules_run_the_same_number_of_steps_at_every_frame_rate() {
	let steps_in_ten_seconds = |frames_a_second: u32| {
		let mut steps = FixedSteps::default();
		(0..10 * frames_a_second)
			.map(|_| steps.advance(1.0 / frames_a_second as f32))
			.sum::<u32>()
	};
	for rate in [30, 60, 75, 144, 240] {
		let steps = steps_in_ten_seconds(rate);
		assert!(
			steps.abs_diff(600) <= 1,
			"{steps} steps in 10 s at {rate} frames a second"
		);
	}
}

#[test]
fn a_long_pause_runs_few_steps_and_drops_the_rest() {
	let mut steps = FixedSteps::default();
	assert_eq!(steps.advance(3.0), tuning::MAX_STEPS_PER_FRAME);
	assert_eq!(steps.advance(tuning::STEP * 1.5), 1, "the dropped time came back");
}

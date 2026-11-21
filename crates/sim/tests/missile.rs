//! The lock, the flight, and the burst of a missile, between two machines on one bus.

mod common;

use std::f32::consts::{FRAC_PI_2, PI};

use common::{Bus, toward};
use glam::{Quat, Vec3};
use sim::{
	Controls, Match, Missile, Part, Plane, burst_damage, tuning,
	wire::{Message, PlaneId, PlaneState, Settings},
};

const SHOOTER: u32 = 1;
const TARGET: u32 = 2;
const NO_BOTS: Settings = Settings {
	bots: 0,
	skill: sim::Skill::Rookie,
};

/// A room in which the shooter flies north at 500 m, and the target starts at `position` with `heading`.
fn duel(position: Vec3, heading: f32) -> Bus {
	let mut bus = Bus::new(&[SHOOTER, TARGET], NO_BOTS);
	bus.place(SHOOTER, Vec3::new(0.0, 500.0, 0.0), Quat::IDENTITY);
	bus.place(TARGET, position, Quat::from_rotation_y(heading));
	bus
}

fn launches(bus: &Bus) -> usize {
	bus.log
		.iter()
		.filter(|(_, message)| matches!(message, Message::Launch { .. }))
		.count()
}

/// The controls of a shooter that holds its nose on the target, and asks for a launch if `launch` is set.
fn hold_nose_on_target(game: &Match, launch: bool) -> Controls {
	let planes = game.planes();
	let position = |id| {
		planes
			.iter()
			.find(|(plane, _)| *plane == id)
			.map(|(_, state)| state.position)
	};
	let aim = match (position(PlaneId::Player(TARGET)), position(PlaneId::Player(SHOOTER))) {
		(Some(target), Some(shooter)) => target - shooter,
		_ => Vec3::NEG_Z,
	};
	Controls {
		launch,
		throttle: 0.5,
		..toward(aim)
	}
}

/// Let the shooter launch one missile at the target, which `pilot` flies. The pilot sees its own
/// plane, and the missile once it flies. Return whether the burst of the missile damaged the target.
fn missile_hits(bus: &mut Bus, pilot: impl Fn(&Plane, Option<&Missile>) -> Controls) -> bool {
	for _ in 0..(30.0 / tuning::STEP) as u32 {
		let launched = launches(bus) > 0;
		bus.step(|number, game| {
			if number == SHOOTER {
				return hold_nose_on_target(game, !launched);
			}
			let plane = game
				.plane(PlaneId::Player(TARGET))
				.expect("the target machine flies the target");
			pilot(plane, game.missiles().first())
		});
		if launched && bus.machine(SHOOTER).missiles().is_empty() {
			break;
		}
	}
	assert_eq!(launches(bus), 1, "the shooter must launch one missile");
	bus.log.iter().any(|(_, message)| {
		matches!(
			message,
			Message::Hit {
				target: PlaneId::Player(TARGET),
				..
			}
		)
	})
}

/// A pilot that flies straight until the missile is nearer than `distance`, and then turns hard toward `escape`.
fn break_at(distance: f32, escape: Vec3) -> impl Fn(&Plane, Option<&Missile>) -> Controls {
	move |plane, missile| {
		let near = missile.is_some_and(|missile| missile.position.distance(plane.position) < distance);
		if near { toward(escape) } else { toward(plane.forward()) }
	}
}

#[test]
fn a_launch_needs_a_lock_that_the_seeker_held_for_the_lock_time() {
	let mut bus = duel(Vec3::new(0.0, 500.0, -400.0), 0.0);
	bus.run(tuning::LOCK_SECONDS - 0.1, |number, game| {
		if number == SHOOTER {
			hold_nose_on_target(game, true)
		} else {
			toward(Vec3::NEG_Z)
		}
	});
	assert_eq!(launches(&bus), 0, "the missile left before the lock closed");
	let (target, closed) = bus
		.machine(SHOOTER)
		.lock(PlaneId::Player(SHOOTER))
		.expect("the seeker follows the target");
	assert_eq!(target, PlaneId::Player(TARGET));
	assert!(closed > 0.8 && closed < 1.0, "the lock closed to {closed}");

	bus.run(0.3, |number, game| {
		if number == SHOOTER {
			hold_nose_on_target(game, true)
		} else {
			toward(Vec3::NEG_Z)
		}
	});
	assert_eq!(launches(&bus), 1);
}

#[test]
fn the_target_sees_the_lock_and_the_missile() {
	let mut bus = duel(Vec3::new(0.0, 500.0, -400.0), 0.0);
	bus.run(0.5, |number, game| {
		if number == SHOOTER {
			hold_nose_on_target(game, false)
		} else {
			toward(Vec3::NEG_Z)
		}
	});
	let planes = bus.machine(TARGET).planes();
	let (_, shooter) = planes
		.iter()
		.find(|(id, _)| *id == PlaneId::Player(SHOOTER))
		.expect("the target knows the shooter");
	assert_eq!(
		shooter.lock,
		Some(PlaneId::Player(TARGET)),
		"the target does not see the lock"
	);

	bus.run(1.5, |number, game| {
		if number == SHOOTER {
			hold_nose_on_target(game, true)
		} else {
			toward(Vec3::NEG_Z)
		}
	});
	let missiles = bus.machine(TARGET).missiles();
	assert_eq!(missiles.len(), 1, "the target does not see the missile");
	assert_eq!(missiles[0].target, PlaneId::Player(TARGET));
}

#[test]
fn a_plane_out_of_the_cone_or_the_range_gets_no_lock() {
	for position in [
		Vec3::new(300.0, 500.0, -100.0),
		Vec3::new(0.0, 500.0, -(tuning::LOCK_RANGE + 300.0)),
	] {
		let mut bus = duel(position, 0.0);
		bus.run(0.2, |_, _| toward(Vec3::NEG_Z));
		assert_eq!(
			bus.machine(SHOOTER).lock(PlaneId::Player(SHOOTER)),
			None,
			"a lock on a plane at {position}"
		);
	}
}

#[test]
fn the_rack_holds_two_missiles_and_fills_one_at_a_time() {
	let mut bus = duel(Vec3::new(0.0, 500.0, -300.0), 0.0);
	bus.machine(TARGET)
		.plane_mut(PlaneId::Player(TARGET))
		.expect("the target")
		.protect(0.0);
	let both = 2.0 * tuning::LOCK_SECONDS + tuning::MISSILE_INTERVAL + 0.5;
	bus.run(both, |number, game| {
		if number == SHOOTER {
			hold_nose_on_target(game, true)
		} else {
			toward(Vec3::NEG_Z)
		}
	});
	assert_eq!(launches(&bus), 2, "two missiles leave, with a new lock for each");
	assert_eq!(bus.machine(SHOOTER).missiles_ready(PlaneId::Player(SHOOTER)), Some(0));

	bus.run(3.0, |number, game| {
		if number == SHOOTER {
			hold_nose_on_target(game, true)
		} else {
			toward(Vec3::NEG_Z)
		}
	});
	assert_eq!(launches(&bus), 2, "an empty rack launched a missile");
}

#[test]
fn a_missile_catches_a_plane_that_flies_straight() {
	let mut bus = duel(Vec3::new(40.0, 500.0, -400.0), 0.3);
	assert!(missile_hits(&mut bus, |plane, _| toward(plane.forward())));
}

#[test]
fn an_early_break_escapes_a_head_on_missile_and_a_late_break_does_not() {
	let mut early = duel(Vec3::new(0.0, 500.0, -1300.0), PI);
	assert!(
		!missile_hits(&mut early, break_at(500.0, Vec3::X)),
		"a break at 500 m must escape"
	);
	let mut late = duel(Vec3::new(0.0, 500.0, -1300.0), PI);
	assert!(
		missile_hits(&mut late, break_at(150.0, Vec3::X)),
		"a break at 150 m must be too late"
	);
}

#[test]
fn a_break_across_the_nose_of_the_missile_escapes_it() {
	// The target crosses from right to left, and turns toward the side from which the missile comes.
	let mut bus = duel(Vec3::new(150.0, 500.0, -700.0), FRAC_PI_2);
	assert!(!missile_hits(&mut bus, break_at(200.0, Vec3::Z)));
}

#[test]
fn a_turn_away_from_the_missile_does_not_escape_it() {
	let mut bus = duel(Vec3::new(150.0, 500.0, -700.0), FRAC_PI_2);
	assert!(missile_hits(&mut bus, break_at(200.0, Vec3::NEG_Z)));
}

#[test]
fn a_plane_that_crosses_without_a_break_is_hit() {
	let mut bus = duel(Vec3::new(150.0, 500.0, -700.0), FRAC_PI_2);
	assert!(missile_hits(&mut bus, |plane, _| toward(plane.forward())));
}

#[test]
fn a_boosting_plane_outruns_a_missile_from_500_m_behind() {
	let mut bus = duel(Vec3::new(0.0, 500.0, -500.0), 0.0);
	assert!(!missile_hits(&mut bus, |plane, _| Controls {
		boost: true,
		..toward(plane.forward())
	}));
}

#[test]
fn a_plane_without_boost_does_not_outrun_a_missile_from_500_m_behind() {
	let mut bus = duel(Vec3::new(0.0, 500.0, -500.0), 0.0);
	assert!(missile_hits(&mut bus, |plane, _| toward(plane.forward())));
}

#[test]
fn a_burst_damages_near_parts_more_than_far_parts() {
	let plane = PlaneState {
		position: Vec3::new(0.0, 300.0, 0.0),
		orientation: Quat::IDENTITY,
		speed: 80.0,
		health: [100, 45, 45, 40, 50],
		alive: true,
		protected: false,
		firing: false,
		boosting: false,
		lock: None,
		kills: 0,
		deaths: 0,
	};
	let at_wing = plane.position + Part::LeftWing.centre();
	let damage = burst_damage(at_wing, &plane);
	let of = |part| damage.iter().find(|(hit, _)| *hit == part).map(|(_, points)| *points);
	assert_eq!(
		of(Part::LeftWing),
		Some(tuning::BURST_DAMAGE),
		"a burst at a part takes the full damage"
	);
	assert!(
		of(Part::LeftWing) >= Some(tuning::WING_HEALTH),
		"a burst at a wing must break it"
	);
	let fuselage = of(Part::Fuselage).expect("the fuselage is in reach");
	assert!(
		fuselage < tuning::BURST_DAMAGE && fuselage >= tuning::BURST_EDGE_DAMAGE,
		"the fuselage takes {fuselage}"
	);
	let far_wing = of(Part::RightWing).expect("the far wing is in reach");
	assert!(
		far_wing < fuselage && far_wing < tuning::WING_HEALTH,
		"the far wing takes {far_wing}, and must hold"
	);
	assert_eq!(
		of(Part::Tail),
		Some(
			tuning::BURST_DAMAGE
				- (tuning::BURST_DAMAGE - tuning::BURST_EDGE_DAMAGE)
					* Part::Tail.centre().distance(Part::LeftWing.centre())
					/ tuning::BURST_RADIUS
		)
	);
	assert!(
		burst_damage(at_wing + Vec3::X * 200.0, &plane).is_empty(),
		"a burst 200 m away damaged the plane"
	);
}

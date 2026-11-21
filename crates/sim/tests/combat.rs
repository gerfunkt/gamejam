//! The gun, the damage, and the end of a plane, between two machines on one bus.

mod common;

use common::{Bus, toward};
use glam::{Quat, Vec3};
use sim::{
	Controls, Event, Part, hit_part, tuning,
	wire::{Message, PlaneId, PlaneState, Settings},
};

const SHOOTER: u32 = 1;
const TARGET: u32 = 2;
const NO_BOTS: Settings = Settings {
	bots: 0,
	skill: sim::Skill::Rookie,
};

/// A room in which the shooter flies 150 m behind the target, on the same line.
fn chase() -> Bus {
	let mut bus = Bus::new(&[SHOOTER, TARGET], NO_BOTS);
	bus.place(TARGET, Vec3::new(0.0, 500.0, 0.0), Quat::IDENTITY);
	bus.place(SHOOTER, Vec3::new(0.0, 500.0, 150.0), Quat::IDENTITY);
	bus
}

fn fire(number: u32) -> Controls {
	Controls {
		fire: number == SHOOTER,
		..toward(Vec3::NEG_Z)
	}
}

fn state_at(position: Vec3) -> PlaneState {
	PlaneState {
		position,
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
	}
}

#[test]
fn a_shot_from_behind_hits_the_tail_first() {
	let target = state_at(Vec3::ZERO);
	assert_eq!(
		hit_part(&target, Vec3::new(0.0, 0.3, 30.0), Vec3::new(0.0, 0.3, -30.0)),
		Some(Part::Tail)
	);
}

#[test]
fn a_shot_at_a_wing_tip_hits_that_wing() {
	let target = state_at(Vec3::new(100.0, 50.0, 0.0));
	let tip = target.position + Vec3::new(-4.5, 0.0, 0.3);
	assert_eq!(
		hit_part(&target, tip + Vec3::Y * 20.0, tip - Vec3::Y * 20.0),
		Some(Part::LeftWing)
	);
}

#[test]
fn a_shot_passes_through_the_place_of_a_broken_part() {
	let mut target = state_at(Vec3::ZERO);
	target.health[Part::LeftWing as usize] = 0;
	let tip = Vec3::new(-4.5, 0.0, 0.3);
	assert_eq!(hit_part(&target, tip + Vec3::Y * 20.0, tip - Vec3::Y * 20.0), None);
}

#[test]
fn a_shot_that_stops_short_or_passes_wide_misses() {
	let target = state_at(Vec3::ZERO);
	assert_eq!(
		hit_part(&target, Vec3::new(0.0, 0.0, 60.0), Vec3::new(0.0, 0.0, 20.0)),
		None,
		"the shot stopped short"
	);
	assert_eq!(
		hit_part(&target, Vec3::new(9.0, 0.0, 30.0), Vec3::new(9.0, 0.0, -30.0)),
		None,
		"the shot passed wide"
	);
}

#[test]
fn a_shot_hits_the_nearest_plane_first() {
	let mut bus = Bus::new(&[3, 2, 1], NO_BOTS);
	bus.place(3, Vec3::new(0.0, 500.0, 30.0), Quat::IDENTITY);
	bus.place(2, Vec3::new(0.0, 500.0, 0.0), Quat::IDENTITY);
	bus.place(1, Vec3::new(0.0, 500.0, -1.0), Quat::IDENTITY);
	bus.run(0.5, |number, _| Controls {
		fire: number == 3,
		..toward(Vec3::NEG_Z)
	});
	let events = bus.machine(3).drain_events();
	let first = events.iter().find_map(|event| match event {
		Event::Hit { target, .. } => Some(*target),
		_ => None,
	});
	assert_eq!(
		first,
		Some(PlaneId::Player(2)),
		"the first hit must be the nearer plane"
	);
}

#[test]
fn the_shooter_sees_its_hit_at_once_and_the_owner_of_the_target_applies_it() {
	let mut bus = chase();
	bus.run(1.5, |number, _| fire(number));

	let seen: Vec<Event> = bus.machine(SHOOTER).drain_events();
	assert!(
		seen.iter().any(|event| matches!(
			event,
			Event::Hit {
				target: PlaneId::Player(TARGET),
				..
			}
		)),
		"the shooter saw no hit: {seen:?}"
	);

	let felt = bus.machine(TARGET).drain_events();
	let applied = felt
		.iter()
		.filter(|event| {
			matches!(
				event,
				Event::Damage {
					plane: PlaneId::Player(TARGET),
					..
				}
			)
		})
		.count();
	let hits = bus
		.log
		.iter()
		.filter(|(_, message)| matches!(message, Message::Hit { .. }))
		.count();
	assert!(hits > 0, "the bus carried no hit");
	assert_eq!(
		applied, hits,
		"the target applied {applied} of the {hits} hits on the bus"
	);

	let target = bus
		.machine(TARGET)
		.plane(PlaneId::Player(TARGET))
		.expect("the target machine flies the target")
		.clone();
	assert!(
		target.is_broken(Part::Tail),
		"eleven hits from behind must break the tail first"
	);
	assert!(
		target.health(Part::Fuselage) < tuning::FUSELAGE_HEALTH,
		"the shots behind the broken tail must reach the fuselage"
	);
}

#[test]
fn the_shooter_learns_the_damage_from_the_state_of_the_target() {
	let mut bus = chase();
	bus.run(1.5, |number, _| fire(number));
	let planes = bus.machine(SHOOTER).planes();
	let (_, target) = planes
		.iter()
		.find(|(id, _)| *id == PlaneId::Player(TARGET))
		.expect("the shooter knows the target");
	let whole = [100, 45, 45, 40, 50];
	assert_ne!(target.health, whole, "the shooter still sees a whole target");
}

#[test]
fn every_machine_counts_the_same_kill_and_the_target_flies_again() {
	let mut bus = chase();
	bus.run(6.0, |number, _| fire(number));

	let kills: Vec<_> = bus
		.log
		.iter()
		.filter(|(_, message)| matches!(message, Message::Kill { .. }))
		.collect();
	assert_eq!(kills.len(), 1, "the bus carried {kills:?}");
	assert_eq!(
		kills[0],
		&(
			TARGET,
			Message::Kill {
				victim: PlaneId::Player(TARGET),
				killer: Some(PlaneId::Player(SHOOTER))
			}
		)
	);

	for number in [SHOOTER, TARGET] {
		let planes = bus.machine(number).planes();
		let score = |id| {
			planes
				.iter()
				.find(|(plane, _)| *plane == id)
				.map(|(_, state)| (state.kills, state.deaths))
		};
		assert_eq!(
			score(PlaneId::Player(SHOOTER)),
			Some((1, 0)),
			"the score of the shooter on machine {number}"
		);
		assert_eq!(
			score(PlaneId::Player(TARGET)),
			Some((0, 1)),
			"the score of the target on machine {number}"
		);
		let events = bus.machine(number).drain_events();
		assert!(events.contains(&Event::Kill {
			victim: PlaneId::Player(TARGET),
			killer: Some(PlaneId::Player(SHOOTER))
		}));
	}
}

#[test]
fn a_destroyed_plane_respawns_whole_and_protected_after_three_seconds() {
	let mut bus = chase();
	bus.machine(TARGET)
		.plane_mut(PlaneId::Player(TARGET))
		.expect("the target")
		.destroy();
	bus.step(|number, _| fire(number));
	let wait = bus
		.machine(TARGET)
		.respawn_in(PlaneId::Player(TARGET))
		.expect("a destroyed plane waits for its respawn");
	assert!((wait - tuning::RESPAWN_SECONDS).abs() < 0.1, "the wait is {wait} s");

	bus.run(tuning::RESPAWN_SECONDS + 0.1, |_, _| toward(Vec3::NEG_Z));
	let machine = bus.machine(TARGET);
	assert_eq!(machine.respawn_in(PlaneId::Player(TARGET)), None);
	let plane = machine.plane(PlaneId::Player(TARGET)).expect("the target");
	assert!(!plane.is_destroyed() && plane.is_protected());
	assert_eq!(plane.health(Part::Fuselage), tuning::FUSELAGE_HEALTH);
}

#[test]
fn a_protected_plane_is_no_target() {
	let mut bus = chase();
	bus.machine(TARGET)
		.plane_mut(PlaneId::Player(TARGET))
		.expect("the target")
		.protect(60.0);
	bus.run(3.0, |number, _| fire(number));
	assert!(bus.log.is_empty(), "the bus carried {:?}", bus.log);
}

#[test]
fn a_crash_after_a_hit_gives_the_kill_to_the_attacker() {
	let mut bus = chase();
	bus.run(0.6, |number, _| fire(number));
	assert!(
		bus.log
			.iter()
			.any(|(_, message)| matches!(message, Message::Hit { .. })),
		"the shooter must hit first"
	);
	bus.place(TARGET, Vec3::new(0.0, 30.0, -2000.0), Quat::from_rotation_x(-1.0));
	let whole = bus
		.machine(TARGET)
		.plane(PlaneId::Player(TARGET))
		.expect("the target")
		.clone();
	assert!(!whole.is_destroyed());
	bus.run(2.0, |number, _| {
		if number == TARGET {
			toward(Vec3::NEG_Y)
		} else {
			toward(Vec3::NEG_Z)
		}
	});
	assert!(
		bus.log.contains(&(
			TARGET,
			Message::Kill {
				victim: PlaneId::Player(TARGET),
				killer: Some(PlaneId::Player(SHOOTER))
			}
		)),
		"the bus carried {:?}",
		bus.log
			.iter()
			.filter(|(_, message)| !matches!(message, Message::Hit { .. }))
			.collect::<Vec<_>>()
	);
}

#[test]
fn a_crash_without_an_attacker_is_a_kill_for_nobody() {
	let mut bus = Bus::new(&[SHOOTER], NO_BOTS);
	bus.place(SHOOTER, Vec3::new(0.0, 30.0, 0.0), Quat::from_rotation_x(-1.0));
	bus.run(2.0, |_, _| toward(Vec3::NEG_Y));
	assert_eq!(
		bus.log,
		[(
			SHOOTER,
			Message::Kill {
				victim: PlaneId::Player(SHOOTER),
				killer: None
			}
		)]
	);
}

#[test]
fn a_machine_ignores_a_state_and_a_kill_for_a_plane_that_the_sender_does_not_fly() {
	let mut bus = chase();
	bus.run(0.2, |_, _| toward(Vec3::NEG_Z));
	let forged = state_at(Vec3::new(9000.0, 9000.0, 9000.0));
	let machine = bus.machine(SHOOTER);
	machine.receive(
		2000.0,
		3,
		&Message::State {
			plane: PlaneId::Player(TARGET),
			clock: 2000,
			state: forged,
		},
	);
	machine.receive(
		2000.0,
		3,
		&Message::Kill {
			victim: PlaneId::Player(TARGET),
			killer: Some(PlaneId::Player(3)),
		},
	);
	let planes = machine.planes();
	let (_, target) = planes
		.iter()
		.find(|(id, _)| *id == PlaneId::Player(TARGET))
		.expect("the shooter knows the target");
	assert!(
		target.position.y < 600.0,
		"the forged state moved the target to {}",
		target.position
	);
	assert_eq!(target.deaths, 0);
	assert!(machine.drain_events().is_empty());
}

#[test]
fn bytes_that_are_no_message_decode_to_nothing() {
	for bytes in [&[][..], &[0xFF; 3], &[7, 1, b'a', 0], &[5, 0], &[0]] {
		assert_eq!(Message::decode(bytes), None, "{bytes:?} decoded");
	}
}

#[test]
fn each_message_survives_the_wire() {
	let messages = [
		Message::State {
			plane: PlaneId::Bot(3),
			clock: 123_456,
			state: state_at(Vec3::new(1.5, -2.0, 3.25)),
		},
		Message::Hit {
			target: PlaneId::Player(4_000_000_000),
			part: Part::Engine,
			damage: 9.0,
			by: PlaneId::Bot(0),
		},
		Message::Kill {
			victim: PlaneId::Player(2),
			killer: None,
		},
		Message::Launch {
			shooter: PlaneId::Player(1),
			missile: 7,
			target: PlaneId::Bot(2),
			position: Vec3::ONE,
			direction: Vec3::X,
		},
		Message::Burst {
			shooter: PlaneId::Player(1),
			missile: 7,
			position: Vec3::NEG_ONE,
		},
		Message::EndMatch,
		Message::Lobby {
			settings: Settings {
				bots: 8,
				skill: sim::Skill::Ace,
			},
			playing: Some(99),
		},
		Message::Profile {
			name: "Zoë 北".into()
		},
	];
	for message in messages {
		assert_eq!(Message::decode(&message.encode()), Some(message.clone()), "{message:?}");
	}
}

#[test]
fn a_state_is_small_enough_to_send_thirty_times_a_second() {
	let state = Message::State {
		plane: PlaneId::Player(12),
		clock: 3_600_000,
		state: state_at(Vec3::new(812.5, 431.25, -977.0)),
	};
	let bytes = state.encode().len();
	assert!(bytes <= 64, "a state takes {bytes} bytes");
}

#[test]
fn a_plane_that_loses_a_wing_falls_and_its_attacker_gets_the_kill() {
	let mut bus = chase();
	let broken_wing = Message::Hit {
		target: PlaneId::Player(TARGET),
		part: Part::LeftWing,
		damage: tuning::WING_HEALTH,
		by: PlaneId::Player(SHOOTER),
	};
	let now = bus.now;
	for number in [SHOOTER, TARGET] {
		bus.machine(number).receive(now, SHOOTER, &broken_wing);
	}
	let target = bus
		.machine(TARGET)
		.plane(PlaneId::Player(TARGET))
		.expect("the target")
		.clone();
	assert!(target.is_falling() && !target.is_destroyed());

	bus.run(tuning::WING_LOSS_SECONDS + 0.2, |_, _| toward(Vec3::NEG_Z));
	let kill = Message::Kill {
		victim: PlaneId::Player(TARGET),
		killer: Some(PlaneId::Player(SHOOTER)),
	};
	assert_eq!(
		bus.log,
		[(TARGET, kill)],
		"the fall must end in one kill for the attacker"
	);
}

#[test]
fn a_bullet_ends_at_the_terrain() {
	let mut bus = Bus::new(&[SHOOTER], NO_BOTS);
	// The shooter dives at the ground, so its bullets reach the ground within their flight.
	bus.place(SHOOTER, Vec3::new(0.0, 330.0, 0.0), Quat::from_rotation_x(-1.2));
	bus.run(0.6, |_, _| Controls {
		fire: true,
		..toward(Vec3::NEG_Y)
	});
	let bullets = bus.machine(SHOOTER).bullets().to_vec();
	assert!(!bullets.is_empty(), "the gun fired no bullet");
	for bullet in bullets {
		let ground = sim::arena::ground_height(bullet.position.x, bullet.position.z);
		assert!(
			bullet.position.y >= ground,
			"a bullet flies {} m under the ground",
			ground - bullet.position.y
		);
	}
}

//! The power-ups of the arena, between machines on one bus.

mod common;

use common::{Bus, toward};
use glam::{Quat, Vec3};
use sim::{
	Controls, Event, Match, Part, Plane, PowerUp, PowerUpKind, arena, tuning,
	wire::{Message, PlaneId, Settings},
};

/// Planes fight from about this height up to the ceiling, in metres.
const FIGHT_FLOOR: f32 = 400.0;
const FIRST: u32 = 1;
const SECOND: u32 = 2;
const NO_BOTS: Settings = Settings {
	bots: 0,
	skill: sim::Skill::Rookie,
};

fn bus(numbers: &[u32]) -> Bus {
	Bus::new(numbers, NO_BOTS)
}

/// The place and the power-up of the first place with this kind.
fn place_of(game: &Match, kind: PowerUpKind) -> (u8, PowerUp) {
	let (place, power_up) = game
		.power_ups()
		.iter()
		.enumerate()
		.find(|(_, power_up)| power_up.kind == kind)
		.expect("the arena holds a power-up of each kind");
	(place as u8, *power_up)
}

/// Put the plane of `number` on the power-up at `place`, and let one step pass.
fn fly_into(bus: &mut Bus, number: u32, place: u8) {
	let position = bus.machine(number).power_ups()[usize::from(place)].position;
	bus.place(number, position, Quat::IDENTITY);
	bus.step(|_, _| toward(Vec3::NEG_Z));
}

fn present(bus: &mut Bus, number: u32, place: u8) -> bool {
	bus.machine(number).power_ups()[usize::from(place)].present
}

fn power_up_events(bus: &mut Bus, number: u32) -> Vec<Event> {
	let events = bus.machine(number).drain_events();
	events
		.into_iter()
		.filter(|event| matches!(event, Event::PowerUp { .. }))
		.collect()
}

fn claims(bus: &Bus) -> usize {
	bus.log
		.iter()
		.filter(|(_, message)| matches!(message, Message::Claim { .. }))
		.count()
}

#[test]
fn the_arena_holds_eight_power_ups_of_three_kinds_in_the_sky() {
	let game = Match::new(FIRST, 7, NO_BOTS);
	let power_ups = game.power_ups();
	assert_eq!(power_ups.len(), 8);
	for kind in [PowerUpKind::Speed, PowerUpKind::Weapons, PowerUpKind::Repair] {
		assert!(power_ups.iter().any(|power_up| power_up.kind == kind), "no {kind:?}");
	}
	for power_up in power_ups {
		let (x, y, z) = (power_up.position.x, power_up.position.y, power_up.position.z);
		assert!(power_up.present, "a new match starts with every power-up there");
		assert!(
			Vec3::new(x, 0.0, z).length() <= tuning::ARENA_RADIUS / 2.0,
			"{x} {z} lies too far out"
		);
		assert!(
			y >= arena::clear_height(x, z) + 150.0,
			"{y} m is too near to the terrain: it does not float in open air"
		);
		assert!(
			(FIGHT_FLOOR..=tuning::CEILING).contains(&y),
			"{y} m is outside the heights of the fight"
		);
	}
}

#[test]
fn every_machine_has_the_same_power_ups() {
	let one = Match::new(FIRST, 7, NO_BOTS);
	let other = Match::new(SECOND, 99, NO_BOTS);
	assert_eq!(one.power_ups(), other.power_ups());
}

#[test]
fn a_plane_that_flies_through_a_power_up_takes_it_and_it_is_gone_on_every_machine() {
	let mut bus = bus(&[FIRST, SECOND]);
	let (place, _) = place_of(bus.machine(FIRST), PowerUpKind::Speed);
	fly_into(&mut bus, FIRST, place);
	for number in [FIRST, SECOND] {
		assert!(!present(&mut bus, number, place), "machine {number} still shows it");
	}
	assert_eq!(claims(&bus), 1, "the plane sends one claim");
}

#[test]
fn a_plane_near_a_power_up_but_outside_its_radius_takes_nothing() {
	let mut bus = bus(&[FIRST]);
	let (place, power_up) = place_of(bus.machine(FIRST), PowerUpKind::Speed);
	let beside = power_up.position + Vec3::X * (tuning::POWERUP_RADIUS + 5.0);
	bus.place(FIRST, beside, Quat::IDENTITY);
	bus.step(|_, _| toward(Vec3::NEG_Z));
	assert!(present(&mut bus, FIRST, place));
	assert_eq!(claims(&bus), 0);
}

#[test]
fn a_taken_power_up_returns_after_the_return_time() {
	let mut bus = bus(&[FIRST, SECOND]);
	let (place, _) = place_of(bus.machine(FIRST), PowerUpKind::Repair);
	fly_into(&mut bus, FIRST, place);
	bus.run(tuning::POWERUP_RETURN_SECONDS - 1.0, |_, _| toward(Vec3::NEG_Z));
	assert!(!present(&mut bus, SECOND, place), "it returned too early");
	bus.run(2.0, |_, _| toward(Vec3::NEG_Z));
	for number in [FIRST, SECOND] {
		assert!(present(&mut bus, number, place), "machine {number} shows no return");
	}
}

#[test]
fn a_plane_takes_a_power_up_once_while_it_stays_in_the_radius() {
	let mut bus = bus(&[FIRST]);
	let (place, power_up) = place_of(bus.machine(FIRST), PowerUpKind::Weapons);
	bus.place(FIRST, power_up.position, Quat::IDENTITY);
	// A plane that hovers in the radius would claim again if the power-up stayed.
	bus.run(1.0, |_, _| toward(Vec3::NEG_Z));
	assert!(!present(&mut bus, FIRST, place));
	assert_eq!(claims(&bus), 1);
}

#[test]
fn two_planes_that_reach_one_power_up_in_the_same_step_have_one_winner_that_every_machine_names() {
	let mut bus = bus(&[FIRST, SECOND]);
	let (place, power_up) = place_of(bus.machine(FIRST), PowerUpKind::Speed);
	bus.place(FIRST, power_up.position, Quat::IDENTITY);
	bus.place(SECOND, power_up.position, Quat::IDENTITY);
	bus.step(|_, _| toward(Vec3::NEG_Z));
	assert_eq!(claims(&bus), 2, "both planes claim it");
	let winner = Event::PowerUp {
		plane: PlaneId::Player(FIRST),
		kind: PowerUpKind::Speed,
	};
	for number in [FIRST, SECOND] {
		assert_eq!(
			power_up_events(&mut bus, number),
			std::slice::from_ref(&winner),
			"machine {number} must see the first claim of the bus win"
		);
	}
	assert!(!present(&mut bus, FIRST, place));
}

#[test]
fn a_claim_for_a_power_up_that_is_gone_does_nothing() {
	let mut bus = bus(&[FIRST]);
	let (place, _) = place_of(bus.machine(FIRST), PowerUpKind::Speed);
	fly_into(&mut bus, FIRST, place);
	let again = Message::Claim {
		place,
		plane: PlaneId::Player(FIRST),
	};
	let now = bus.now;
	bus.machine(FIRST).receive(now, FIRST, &again);
	assert_eq!(
		power_up_events(&mut bus, FIRST).len(),
		1,
		"the second claim made an event"
	);
}

#[test]
fn a_destroyed_plane_takes_nothing() {
	let mut bus = bus(&[FIRST]);
	let (place, power_up) = place_of(bus.machine(FIRST), PowerUpKind::Speed);
	bus.place(FIRST, power_up.position, Quat::IDENTITY);
	bus.machine(FIRST)
		.plane_mut(PlaneId::Player(FIRST))
		.expect("the plane")
		.destroy();
	bus.step(|_, _| toward(Vec3::NEG_Z));
	assert!(present(&mut bus, FIRST, place));
	assert_eq!(claims(&bus), 0);
}

#[test]
fn a_plane_that_falls_after_a_lost_wing_takes_nothing() {
	let mut bus = bus(&[FIRST]);
	let (place, power_up) = place_of(bus.machine(FIRST), PowerUpKind::Repair);
	bus.place(FIRST, power_up.position, Quat::IDENTITY);
	let plane = bus.machine(FIRST).plane_mut(PlaneId::Player(FIRST)).expect("the plane");
	plane.damage(Part::LeftWing, tuning::WING_HEALTH);
	assert!(plane.is_falling());
	bus.step(|_, _| toward(Vec3::NEG_Z));
	assert!(present(&mut bus, FIRST, place));
	assert_eq!(claims(&bus), 0);
}

#[test]
fn a_claim_from_a_member_that_does_not_fly_the_plane_is_ignored() {
	let mut bus = bus(&[FIRST, SECOND]);
	let (place, _) = place_of(bus.machine(FIRST), PowerUpKind::Speed);
	let now = bus.now;
	// The second member claims for the plane of the first, and for a bot that only the host flies.
	for plane in [PlaneId::Player(FIRST), PlaneId::Bot(0)] {
		let forged = Message::Claim { place, plane };
		for number in [FIRST, SECOND] {
			bus.machine(number).receive(now, SECOND, &forged);
		}
	}
	for number in [FIRST, SECOND] {
		assert!(present(&mut bus, number, place), "machine {number} took a forged claim");
		assert!(power_up_events(&mut bus, number).is_empty());
	}
}

#[test]
fn a_claim_for_a_place_that_does_not_exist_is_ignored() {
	let mut game = Match::new(FIRST, 7, NO_BOTS);
	let claim = Message::Claim {
		place: u8::MAX,
		plane: PlaneId::Player(FIRST),
	};
	game.receive(0.0, FIRST, &claim);
	assert!(game.drain_events().is_empty());
}

#[test]
fn a_weapons_power_up_fills_the_missile_rack_and_the_flare_rack_at_once() {
	let mut bus = bus(&[FIRST, SECOND]);
	bus.place(FIRST, Vec3::new(0.0, 500.0, 0.0), Quat::IDENTITY);
	bus.place(SECOND, Vec3::new(0.0, 500.0, -300.0), Quat::IDENTITY);
	bus.machine(SECOND)
		.plane_mut(PlaneId::Player(SECOND))
		.expect("the target")
		.protect(0.0);
	let empty_the_racks = |_: u32, _: &Match| Controls {
		launch: true,
		flare: true,
		throttle: 0.5,
		..toward(Vec3::NEG_Z)
	};
	bus.run(4.0, |number, game| {
		if number == FIRST {
			empty_the_racks(number, game)
		} else {
			toward(Vec3::NEG_Z)
		}
	});
	let me = PlaneId::Player(FIRST);
	assert_eq!(
		bus.machine(FIRST).missiles_ready(me),
		Some(0),
		"the missiles are not gone"
	);
	assert_eq!(bus.machine(FIRST).flares_ready(me), Some(0), "the flares are not gone");

	let (place, _) = place_of(bus.machine(FIRST), PowerUpKind::Weapons);
	fly_into(&mut bus, FIRST, place);
	assert_eq!(bus.machine(FIRST).missiles_ready(me), Some(tuning::MISSILES));
	assert_eq!(bus.machine(FIRST).flares_ready(me), Some(tuning::FLARES));
}

fn healths(game: &Match) -> Vec<f32> {
	let plane = game.plane(PlaneId::Player(FIRST)).expect("the plane");
	Part::ALL.map(|part| plane.health(part)).to_vec()
}

#[test]
fn a_repair_power_up_makes_every_part_whole_again() {
	let mut bus = bus(&[FIRST]);
	let (place, power_up) = place_of(bus.machine(FIRST), PowerUpKind::Repair);
	bus.place(FIRST, power_up.position, Quat::IDENTITY);
	let plane = bus.machine(FIRST).plane_mut(PlaneId::Player(FIRST)).expect("the plane");
	plane.damage(Part::Fuselage, 60.0);
	plane.damage(Part::Tail, tuning::TAIL_HEALTH);
	plane.damage(Part::Engine, tuning::ENGINE_HEALTH);
	bus.step(|_, _| toward(Vec3::NEG_Z));
	assert!(!present(&mut bus, FIRST, place));
	assert_eq!(healths(bus.machine(FIRST)), Part::ALL.map(Part::full_health));
}

#[test]
fn a_repair_that_arrives_after_the_loss_of_a_wing_does_not_repair() {
	let mut bus = bus(&[FIRST]);
	let (place, _) = place_of(bus.machine(FIRST), PowerUpKind::Repair);
	let plane = bus.machine(FIRST).plane_mut(PlaneId::Player(FIRST)).expect("the plane");
	plane.damage(Part::LeftWing, tuning::WING_HEALTH);
	let before = healths(bus.machine(FIRST));
	let claim = Message::Claim {
		place,
		plane: PlaneId::Player(FIRST),
	};
	let now = bus.now;
	bus.machine(FIRST).receive(now, FIRST, &claim);
	assert_eq!(healths(bus.machine(FIRST)), before, "a falling plane got a repair");
	assert!(!present(&mut bus, FIRST, place), "the power-up is gone all the same");
}

fn plane() -> Plane {
	Plane::new(Vec3::new(0.0, 500.0, 0.0), Quat::IDENTITY)
}

fn fly(plane: &mut Plane, controls: &Controls, seconds: f32) {
	for _ in 0..(seconds / tuning::STEP).round() as u32 {
		plane.step(controls, tuning::STEP);
	}
}

#[test]
fn a_speed_power_up_raises_the_level_speed_until_its_seconds_run_out() {
	let mut plane = plane();
	plane.grant_speed_up(tuning::POWERUP_SPEED_SECONDS);
	fly(&mut plane, &toward(Vec3::NEG_Z), tuning::POWERUP_SPEED_SECONDS - 1.0);
	let fast = tuning::MAX_SPEED + tuning::POWERUP_SPEED_BONUS;
	assert!((plane.speed - fast).abs() < 0.5, "speed {} is not {fast}", plane.speed);
	assert!(plane.speed_up_left() > 0.5 && plane.speed_up_left() < 1.5);

	fly(&mut plane, &toward(Vec3::NEG_Z), 8.0);
	assert_eq!(plane.speed_up_left(), 0.0);
	assert!((plane.speed - tuning::MAX_SPEED).abs() < 0.5, "speed {}", plane.speed);
}

#[test]
fn boost_costs_nothing_while_a_speed_power_up_lasts() {
	let boost = Controls {
		boost: true,
		..toward(Vec3::NEG_Z)
	};
	let mut plane = plane();
	plane.grant_speed_up(tuning::POWERUP_SPEED_SECONDS);
	fly(&mut plane, &boost, tuning::BOOST_SECONDS + 1.0);
	assert!(plane.is_boosting(), "the boost ran out under the power-up");
	assert_eq!(plane.boost(), 1.0, "the tank emptied");

	fly(&mut plane, &boost, tuning::POWERUP_SPEED_SECONDS);
	assert!(plane.boost() < 1.0, "the boost stayed free after the power-up ended");
}

#[test]
fn a_speed_power_up_speeds_up_the_plane_that_took_it() {
	let mut bus = bus(&[FIRST, SECOND]);
	let (place, _) = place_of(bus.machine(FIRST), PowerUpKind::Speed);
	fly_into(&mut bus, FIRST, place);
	let left = |bus: &mut Bus, number| {
		bus.machine(number)
			.plane(PlaneId::Player(number))
			.map(Plane::speed_up_left)
	};
	let first = left(&mut bus, FIRST).expect("the plane");
	assert!(first > tuning::POWERUP_SPEED_SECONDS - 0.5, "{first} s left");
	assert_eq!(left(&mut bus, SECOND), Some(0.0), "the other plane got nothing");
}

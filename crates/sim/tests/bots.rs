//! The pilot of a bot, alone and in whole fights on one machine.

mod common;

use common::{Bus, toward};
use glam::{Quat, Vec2, Vec3};
use sim::{
	Controls, Plane, Skill, aim_direction, bot_controls, tuning,
	wire::{Message, PlaneId, PlaneState, Settings},
};

const ME: PlaneId = PlaneId::Bot(0);
const OTHER: PlaneId = PlaneId::Player(1);

fn state(position: Vec3) -> PlaneState {
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

/// The controls of a bot at 500 m that flies north and sees one other plane at `position`.
fn bot_that_sees(other: PlaneState, skill: Skill) -> Controls {
	let me = state(Vec3::new(0.0, 500.0, 0.0));
	bot_controls(ME, &me, &[(ME, me), (OTHER, other)], skill)
}

#[test]
fn a_bot_turns_toward_a_target_at_its_side() {
	let controls = bot_that_sees(state(Vec3::new(300.0, 500.0, -300.0)), Skill::Pilot);
	assert!(
		controls.aim.x > 0.1,
		"the aim {} does not lie to the right",
		controls.aim
	);
}

#[test]
fn a_bot_fires_only_at_a_near_target_ahead() {
	assert!(
		bot_that_sees(state(Vec3::new(0.0, 500.0, -250.0)), Skill::Pilot).fire,
		"a near target ahead gets fire"
	);
	assert!(
		!bot_that_sees(state(Vec3::new(0.0, 500.0, -900.0)), Skill::Pilot).fire,
		"900 m is too far"
	);
	assert!(
		!bot_that_sees(state(Vec3::new(0.0, 500.0, 250.0)), Skill::Pilot).fire,
		"a plane behind is no target for the gun"
	);
	assert!(
		!bot_that_sees(state(Vec3::new(120.0, 500.0, -250.0)), Skill::Pilot).fire,
		"the aim is not on the target yet"
	);
}

#[test]
fn a_bot_leaves_a_protected_plane_alone() {
	let protected = PlaneState {
		protected: true,
		..state(Vec3::new(0.0, 500.0, -250.0))
	};
	let controls = bot_that_sees(protected, Skill::Ace);
	assert!(!controls.fire && !controls.launch, "a bot shot at a protected plane");
}

#[test]
fn a_rookie_carries_no_missiles_and_the_other_skills_do() {
	let near = state(Vec3::new(0.0, 500.0, -300.0));
	assert!(!bot_that_sees(near, Skill::Rookie).launch);
	for skill in [Skill::Pilot, Skill::Veteran, Skill::Ace] {
		assert!(bot_that_sees(near, skill).launch, "{skill:?} does not ask for a launch");
	}
}

#[test]
fn a_bot_climbs_away_from_the_ground_before_it_fights() {
	let low = state(Vec3::new(0.0, 60.0, 0.0));
	let below = state(Vec3::new(0.0, 20.0, -250.0));
	let controls = bot_controls(ME, &low, &[(ME, low), (OTHER, below)], Skill::Ace);
	assert!(controls.aim.y > 0.3, "the aim {} does not climb", controls.aim);
}

#[test]
fn a_bot_without_a_target_circles_the_centre() {
	let mut bus = Bus::new(
		&[1],
		Settings {
			bots: 1,
			skill: Skill::Pilot,
		},
	);
	bus.machine(1)
		.plane_mut(PlaneId::Player(1))
		.expect("the player")
		.protect(f32::MAX);
	let mut farthest: f32 = 0.0;
	for _ in 0..(90.0 / tuning::STEP) as u32 {
		bus.step(|_, _| toward(Vec3::NEG_Z));
		let bot = bus.machine(1).plane(PlaneId::Bot(0)).expect("the host flies the bot");
		assert!(!bot.is_destroyed(), "the idle bot crashed");
		farthest = farthest.max(Vec2::new(bot.position.x, bot.position.z).length());
	}
	assert!(farthest < 1200.0, "the idle bot flew {farthest} m away from the centre");
}

/// Let one bot of `skill` chase the player for 40 s, and return the number of its gun hits.
///
/// The player flies slowly, and holds its cursor at `cursor` right of the centre. The test makes
/// the player whole again in each step, so that the chase never ends.
fn gun_hits_in_a_chase(skill: Skill, cursor: f32) -> usize {
	let mut bus = Bus::new(&[1], Settings { bots: 1, skill });
	bus.place(1, Vec3::new(0.0, 600.0, 0.0), Quat::IDENTITY);
	*bus.machine(1).plane_mut(PlaneId::Bot(0)).expect("the bot") =
		Plane::new(Vec3::new(0.0, 600.0, 300.0), Quat::IDENTITY);
	for _ in 0..(40.0 / tuning::STEP) as u32 {
		bus.step(|_, game| {
			let player = game.plane(PlaneId::Player(1)).expect("the player");
			Controls {
				throttle: 0.0,
				..toward(aim_direction(player.orientation, Vec2::new(cursor, 0.0)))
			}
		});
		bus.machine(1)
			.plane_mut(PlaneId::Player(1))
			.expect("the player")
			.repair();
	}
	let gun = |message: &Message| matches!(message, Message::Hit { by: PlaneId::Bot(0), damage, .. } if *damage == tuning::BULLET_DAMAGE);
	bus.log.iter().filter(|(_, message)| gun(message)).count()
}

#[test]
fn a_better_bot_scores_more_gun_hits_on_a_plane_in_a_gentle_turn() {
	let hits = Skill::ALL.map(|skill| gun_hits_in_a_chase(skill, 0.08));
	println!("gun hits in a gentle turn, from rookie to ace: {hits:?}");
	assert!(hits[0] > 0, "a rookie never hits");
	assert!(hits.is_sorted(), "the hits do not rise with the skill: {hits:?}");
	assert!(
		hits[3] > 2 * hits[0],
		"an ace scores {} hits and a rookie {}",
		hits[3],
		hits[0]
	);
}

#[test]
fn a_tight_turn_escapes_every_bot() {
	for skill in Skill::ALL {
		let gentle = gun_hits_in_a_chase(skill, 0.08);
		let tight = gun_hits_in_a_chase(skill, 2.0);
		println!("{skill:?}: {gentle} gun hits in a gentle turn, {tight} in a tight turn");
		assert!(
			tight * 10 <= gentle.max(50),
			"{skill:?} scores {tight} gun hits in a tight turn, and {gentle} in a gentle one"
		);
	}
}

#[test]
fn bots_shoot_each_other_down_and_stay_out_of_the_ground() {
	for skill in Skill::ALL {
		let mut bus = Bus::new(&[1], Settings { bots: 4, skill });
		bus.machine(1)
			.plane_mut(PlaneId::Player(1))
			.expect("the player")
			.protect(f32::MAX);
		bus.run(180.0, |_, _| toward(Vec3::new(0.3, 0.0, -1.0)));
		let kills: Vec<_> = bus
			.log
			.iter()
			.filter_map(|(_, message)| match message {
				Message::Kill { killer, .. } => Some(*killer),
				_ => None,
			})
			.collect();
		let crashes = kills.iter().filter(|killer| killer.is_none()).count();
		println!(
			"{skill:?}: {} kills in 3 minutes, {crashes} of them crashes without an attacker",
			kills.len()
		);
		assert!(
			kills.len() - crashes >= 2,
			"{skill:?} bots made {} kills in 3 minutes",
			kills.len() - crashes
		);
		assert_eq!(crashes, 0, "{skill:?} bots flew into the ground");
	}
}

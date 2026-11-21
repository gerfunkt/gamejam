//! The lobby, the match, and the results of a room, on several machines.

mod common;

use common::{Room, toward};
use glam::{Quat, Vec3};
use sim::{
	Controls, Event, Phase, Plane, Session, Skill, bot_controls,
	wire::{MAX_BOTS, Message, PlaneId, Settings},
};

const ACES: Settings = Settings {
	bots: 3,
	skill: Skill::Ace,
};
const NO_BOTS: Settings = Settings {
	bots: 0,
	skill: Skill::Pilot,
};

/// The controls of a player that a strong bot flies, so that a test match has real fights.
fn fight(_: u32, session: &Session) -> Controls {
	let game = session.game().expect("a match runs");
	let planes = game.planes();
	match planes.iter().find(|(id, _)| *id == game.me()) {
		Some((me, state)) => bot_controls(*me, state, &planes, game.missiles(), game.power_ups(), Skill::Ace),
		None => toward(Vec3::NEG_Z),
	}
}

fn kills(session: &mut Session) -> Vec<Event> {
	let events = session.game_mut().expect("a match runs").drain_events();
	events
		.into_iter()
		.filter(|event| matches!(event, Event::Kill { .. }))
		.collect()
}

#[test]
fn a_solo_session_runs_from_the_lobby_through_a_match_to_the_results() {
	let mut session = Session::solo("Ada");
	session.deliver_to_self(0.0);
	assert_eq!(session.phase(), Phase::Lobby);
	assert!(session.is_host());

	session.set_settings(Settings {
		bots: 2,
		skill: Skill::Veteran,
	});
	session.start_match(11);
	session.deliver_to_self(0.0);
	assert_eq!(session.phase(), Phase::Match);
	let planes = session.game().expect("a match runs").planes();
	assert_eq!(planes.len(), 3, "the player and two bots fly");

	session.request_end();
	session.deliver_to_self(0.0);
	assert_eq!(session.phase(), Phase::Results);
	let names: Vec<&str> = session
		.results()
		.expect("the results show")
		.iter()
		.map(|score| score.name.as_str())
		.collect();
	assert_eq!(names, ["Ada", "Bot 1", "Bot 2"]);

	session.close_results();
	assert_eq!(session.phase(), Phase::Lobby);
	assert_eq!(
		session.settings(),
		Settings {
			bots: 2,
			skill: Skill::Veteran
		},
		"the lobby forgot the choice of the host"
	);
}

#[test]
fn the_lobby_holds_at_most_eight_bots() {
	let mut session = Session::solo("Ada");
	session.set_settings(Settings {
		bots: 200,
		skill: Skill::Rookie,
	});
	assert_eq!(session.settings().bots, MAX_BOTS);
}

#[test]
fn every_member_knows_the_names_and_a_player_without_a_name_gets_one() {
	let mut room = Room::new();
	let zoe = room.join("  Zoë Åström-Müller the Third  ");
	let nameless = room.join("");
	let late = room.join("北");
	for number in [zoe, nameless, late] {
		let players = room.session(number).players();
		let expected = [
			(zoe, "Zoë Åström-Mülle".to_owned()),
			(nameless, format!("Player {nameless}")),
			(late, "北".to_owned()),
		];
		assert_eq!(players, expected, "the names on the machine of member {number}");
	}
}

#[test]
fn only_the_host_changes_the_bots_and_starts_the_match() {
	let mut room = Room::new();
	let host = room.join("Host");
	let guest = room.join("Guest");
	assert!(room.session(host).is_host() && !room.session(guest).is_host());

	room.session(guest).set_settings(ACES);
	room.session(guest).start_match(5);
	room.deliver();
	assert_eq!(
		room.session(host).settings(),
		Settings::default(),
		"a guest changed the bots"
	);
	assert_eq!(room.session(host).phase(), Phase::Lobby, "a guest started the match");

	room.session(host).set_settings(ACES);
	room.deliver();
	assert_eq!(
		room.session(guest).settings(),
		ACES,
		"the guest does not see the choice of the host"
	);
	room.session(host).start_match(5);
	room.deliver();
	assert_eq!(room.session(host).phase(), Phase::Match);
	assert_eq!(room.session(guest).phase(), Phase::Match);
}

#[test]
fn a_member_ignores_a_lobby_message_and_an_end_from_a_guest() {
	let mut room = Room::new();
	let host = room.join("Host");
	let guest = room.join("Guest");
	let forged = Message::Lobby {
		settings: ACES,
		playing: Some(5),
	};
	room.session(host).receive(0.0, guest, &forged);
	assert_eq!(room.session(host).phase(), Phase::Lobby);
	assert_eq!(room.session(host).settings(), Settings::default());

	room.session(host).start_match(5);
	room.deliver();
	room.session(host).receive(0.0, guest, &Message::EndMatch);
	assert_eq!(room.session(host).phase(), Phase::Match, "a guest ended the match");
}

#[test]
fn only_the_host_flies_the_bots_and_every_machine_sees_them() {
	let mut room = Room::new();
	let host = room.join("Host");
	let guest = room.join("Guest");
	room.session(host).set_settings(ACES);
	room.session(host).start_match(5);
	room.run(1.0, |_, _| toward(Vec3::NEG_Z));
	for bot in (0..3).map(PlaneId::Bot) {
		assert!(
			room.session(host).game().expect("a match").plane(bot).is_some(),
			"the host does not fly {bot:?}"
		);
		assert!(
			room.session(guest).game().expect("a match").plane(bot).is_none(),
			"a guest flies {bot:?}"
		);
	}
	assert_eq!(
		room.session(guest).game().expect("a match").planes().len(),
		5,
		"the guest sees two players and three bots"
	);
}

#[test]
fn a_player_that_comes_later_joins_the_running_match() {
	let mut room = Room::new();
	let host = room.join("Host");
	room.session(host).set_settings(ACES);
	room.session(host).start_match(5);
	room.run(2.0, |_, _| toward(Vec3::NEG_Z));

	let late = room.join("Late");
	assert_eq!(
		room.session(late).phase(),
		Phase::Match,
		"the late player waits in the lobby"
	);
	assert_eq!(room.session(late).settings(), ACES);
	room.run(1.0, |_, _| toward(Vec3::NEG_Z));
	assert_eq!(room.session(late).game().expect("a match").planes().len(), 5);
	assert_eq!(
		room.session(host).game().expect("a match").planes().len(),
		5,
		"the host does not see the late player"
	);
	assert_eq!(room.session(late).players().len(), 2);
}

#[test]
fn every_machine_counts_the_same_kills_in_the_same_order() {
	let mut room = Room::new();
	let numbers = [room.join("A"), room.join("B"), room.join("C")];
	room.session(numbers[0]).set_settings(ACES);
	room.session(numbers[0]).start_match(5);
	room.deliver();
	let mut seen: Vec<Vec<Event>> = vec![Vec::new(); 3];
	for _ in 0..120 {
		room.run(1.0, fight);
		for (index, number) in numbers.into_iter().enumerate() {
			seen[index].extend(kills(room.session(number)));
		}
	}
	assert!(
		seen[0].len() >= 5,
		"two minutes with six planes gave only {} kills",
		seen[0].len()
	);
	assert_eq!(seen[0], seen[1], "machines A and B disagree");
	assert_eq!(seen[0], seen[2], "machines A and C disagree");
}

#[test]
fn every_machine_shows_the_same_results_when_the_host_ends_the_match() {
	let mut room = Room::new();
	let shooter = room.join("Shooter");
	let target = room.join("Target");
	room.session(shooter).set_settings(NO_BOTS);
	room.session(shooter).start_match(5);
	room.deliver();
	let place = |room: &mut Room, number: u32, z: f32| {
		let game = room.session(number).game_mut().expect("a match");
		*game
			.plane_mut(PlaneId::Player(number))
			.expect("each machine flies its player") = Plane::new(Vec3::new(0.0, 500.0, z), Quat::IDENTITY);
	};
	place(&mut room, target, 0.0);
	place(&mut room, shooter, 150.0);
	room.run(6.0, |number, _| Controls {
		fire: number == shooter,
		..toward(Vec3::NEG_Z)
	});
	room.run(1.0, |_, _| toward(Vec3::NEG_Z));

	room.session(target).request_end();
	room.deliver();
	assert_eq!(room.session(target).phase(), Phase::Match, "a guest ended the match");
	room.session(shooter).request_end();
	room.deliver();

	let results = room
		.session(shooter)
		.results()
		.expect("the host sees the results")
		.to_vec();
	let line = |index: usize| {
		(
			results[index].name.as_str(),
			results[index].kills,
			results[index].deaths,
		)
	};
	assert_eq!((line(0), line(1)), (("Shooter", 1, 0), ("Target", 0, 1)));
	assert_eq!(
		room.session(target).results(),
		Some(&results[..]),
		"the guest sees other results"
	);
	assert_eq!(room.session(target).phase(), Phase::Results);
}

#[test]
fn the_next_member_hosts_the_lobby_when_the_host_leaves() {
	let mut room = Room::new();
	let host = room.join("Host");
	let second = room.join("Second");
	let third = room.join("Third");
	room.session(host).set_settings(ACES);
	room.deliver();
	room.leave(host);
	assert!(room.session(second).is_host() && !room.session(third).is_host());
	assert_eq!(
		room.session(second).settings(),
		ACES,
		"the new host lost the choice of the old one"
	);

	room.session(second).start_match(9);
	room.deliver();
	assert_eq!(room.session(third).phase(), Phase::Match);
}

#[test]
fn the_next_member_flies_the_bots_on_when_the_host_leaves_during_a_match() {
	let mut room = Room::new();
	let host = room.join("Host");
	let second = room.join("Second");
	let third = room.join("Third");
	room.session(host).set_settings(ACES);
	room.session(host).start_match(5);
	room.run(3.0, |_, _| toward(Vec3::NEG_Z));
	let before = room
		.session(host)
		.game()
		.expect("a match")
		.plane(PlaneId::Bot(0))
		.expect("the host flies the bots")
		.position;

	room.leave(host);
	let game = room.session(second).game().expect("the match continues");
	let after = game
		.plane(PlaneId::Bot(0))
		.expect("the new host flies the bots")
		.position;
	assert!(before.distance(after) < 30.0, "the bot jumped from {before} to {after}");
	assert_eq!(game.planes().len(), 5, "two players and three bots stay");

	room.run(60.0, fight);
	let bot_kills = room
		.log
		.iter()
		.filter(|(sender, message)| {
			*sender == second
				&& matches!(
					message,
					Message::Kill {
						victim: PlaneId::Bot(_),
						..
					}
				)
		})
		.count();
	let on_third = room.session(third).game().expect("the match continues").planes().len();
	assert_eq!(on_third, 5, "the guest lost planes");
	assert!(
		room.log.iter().any(|(_, message)| matches!(
			message,
			Message::Hit {
				by: PlaneId::Bot(_),
				..
			}
		)),
		"the bots stopped their fight"
	);
	println!("the new host announced {bot_kills} ends of bots");
}

#[test]
fn the_planes_of_a_member_that_left_disappear() {
	let mut room = Room::new();
	let host = room.join("Host");
	let guest = room.join("Guest");
	room.session(host).set_settings(NO_BOTS);
	room.session(host).start_match(5);
	room.run(1.0, |_, _| toward(Vec3::NEG_Z));
	assert_eq!(room.session(host).game().expect("a match").planes().len(), 2);
	room.leave(guest);
	room.run(0.5, |_, _| toward(Vec3::NEG_Z));
	assert_eq!(room.session(host).game().expect("a match").planes().len(), 1);
}

#[test]
fn the_host_renames_the_room_and_removes_a_member_and_a_guest_cannot() {
	let mut room = Room::new();
	let host = room.join("Host");
	let guest = room.join("Guest");
	room.session(guest).rename_room("Mine");
	room.session(guest).remove_member(host);
	room.deliver();
	assert_eq!(room.session(host).room().title, "Room 1");
	assert_eq!(room.sessions.len(), 2);

	room.session(host).rename_room("Friday night");
	room.deliver();
	assert_eq!(room.session(guest).room().title, "Friday night");
	room.session(host).remove_member(host);
	room.session(host).remove_member(guest);
	room.deliver();
	assert_eq!(room.sessions.len(), 1, "the host must remove the guest, and not itself");
}

#[test]
fn the_host_tells_the_room_list_what_the_room_does() {
	let mut room = Room::new();
	let host = room.join("Host");
	assert_eq!(room.info, "In the lobby, 4 bots (Pilot)");
	room.session(host).set_settings(Settings {
		bots: 1,
		skill: Skill::Ace,
	});
	room.deliver();
	assert_eq!(room.info, "In the lobby, 1 bot (Ace)");
	room.session(host).start_match(5);
	room.deliver();
	assert_eq!(room.info, "In a match, 1 bot (Ace)");
	room.session(host).request_end();
	room.deliver();
	assert_eq!(room.info, "In the lobby, 1 bot (Ace)");
}

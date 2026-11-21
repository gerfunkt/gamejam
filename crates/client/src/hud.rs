//! What the page shows around the sky: the menus, and the display of a match.

use bevy::prelude::*;
use sim::{
	Event, Part, Phase, Session, Skill,
	room::MAX_MEMBERS,
	tuning,
	wire::{MAX_BOTS, PlaneId},
};

use crate::{
	flow::{Flow, Game, Screen},
	input::{Bind, key_name},
	page::{self, Piece},
	view::Drawn,
};

/// The most planes that the display marks at once.
const MARKERS: usize = 16;
/// How long a line of the kill feed stays, in seconds.
const FEED_SECONDS: f32 = 7.0;
/// How long the gun sight shows a hit, in seconds.
const HIT_SECONDS: f32 = 0.12;
/// The distance ahead of the plane at which the gun sight floats, in metres.
const SIGHT_DISTANCE: f32 = 400.0;

const PART_NAMES: [&str; Part::ALL.len()] = ["Fuselage", "Left Wing", "Right Wing", "Tail", "Engine"];

pub fn plugin(app: &mut App) {
	// The page gets the rows that never change once: the bars of the parts, and the markers.
	let bars: Vec<String> = (0..Part::ALL.len()).map(|index| format!("bar-{index}")).collect();
	let bar_attributes: Vec<[(&str, &str); 2]> =
		bars.iter().map(|id| [("class", "bar"), ("id", id.as_str())]).collect();
	let rows: Vec<(&str, Vec<Piece>)> = PART_NAMES
		.iter()
		.zip(&bar_attributes)
		.map(|(name, attributes)| {
			let bar = Piece {
				tag: "dd",
				text: "",
				attributes,
			};
			(
				"",
				vec![
					Piece {
						tag: "dt",
						text: name,
						attributes: &[],
					},
					bar,
				],
			)
		})
		.collect();
	page::set_rows("parts", "div", &rows);
	let markers: Vec<String> = (0..MARKERS).map(|index| format!("marker-{index}")).collect();
	let marker_attributes: Vec<[(&str, &str); 2]> = markers
		.iter()
		.map(|id| [("id", id.as_str()), ("class", "marker")])
		.collect();
	let pool = marker_attributes
		.iter()
		.map(|attributes| Piece {
			tag: "div",
			text: "",
			attributes,
		})
		.collect();
	page::set_rows("markers", "div", &[("", pool)]);
	for id in &markers {
		page::set_hidden(id, true);
	}
	app.add_systems(Update, (show_screens, show_display).chain().after(Flow).after(Drawn));
}

fn show_screens(game: Res<Game>) {
	page::set_text("status", "");
	let screens = [
		("menu", Screen::Menu),
		("rooms", Screen::Rooms),
		("lobby", Screen::Lobby),
		("settings", Screen::Settings),
		("pause", Screen::Paused),
		("results", Screen::Results),
		("lost", Screen::Lost),
	];
	for (id, screen) in screens {
		page::set_hidden(id, game.screen != screen);
	}
	match game.screen {
		Screen::Menu => page::set_hidden("fallback", page::backend() != "webgl2"),
		Screen::Rooms => show_rooms(&game),
		Screen::Lobby => show_lobby(&game),
		Screen::Settings => show_settings(&game),
		Screen::Paused => {
			let host = game.session.as_ref().is_some_and(Session::is_host);
			page::set_text("pause-title", if game.online { "Menu" } else { "Paused" });
			page::set_text(
				"pause-note",
				if game.online {
					"The match runs on while this menu is open."
				} else {
					"The match waits for you."
				},
			);
			page::set_hidden("end", !host);
			page::set_text(
				"end",
				if game.is_armed("end") {
					"Confirm: End Match"
				} else {
					"End Match"
				},
			);
		}
		Screen::Results => {
			let scores = game.session.as_ref().and_then(Session::results).unwrap_or_default();
			let texts: Vec<(String, String)> = scores
				.iter()
				.map(|score| (score.kills.to_string(), score.deaths.to_string()))
				.collect();
			let rows: Vec<(&str, Vec<Piece>)> = scores
				.iter()
				.zip(&texts)
				.map(|(score, (kills, deaths))| {
					let cell = |text| Piece {
						tag: "td",
						text,
						attributes: &[],
					};
					("", vec![cell(score.name.as_str()), cell(kills), cell(deaths)])
				})
				.collect();
			page::set_rows("result-rows", "tr", &rows);
		}
		Screen::Lost => page::set_text("lost-note", &game.lost),
		Screen::Playing => {}
	}
}

fn show_rooms(game: &Game) {
	let rooms = match &game.rooms {
		None => {
			page::set_text("rooms-note", "Loading the room list…");
			return page::set_rows("room-list", "li", &[]);
		}
		Some(Err(reason)) => {
			page::set_text("rooms-note", reason);
			return page::set_rows("room-list", "li", &[]);
		}
		Some(Ok(rooms)) => rooms,
	};
	page::set_text(
		"rooms-note",
		"Join a room, or create one. The player who creates a room hosts it.",
	);
	let notes: Vec<String> = rooms
		.iter()
		.map(|room| {
			let doing = if room.info.is_empty() {
				String::new()
			} else {
				format!(" · {}", room.info)
			};
			format!("{} of {MAX_MEMBERS}{doing}", room.members)
		})
		.collect();
	let buttons: Vec<Vec<(&str, &str)>> = rooms
		.iter()
		.map(|room| {
			let mut attributes = vec![
				("type", "button"),
				("data-action", "join"),
				("data-value", room.id.as_str()),
				("class", "quiet"),
			];
			if room.members >= MAX_MEMBERS {
				attributes.push(("disabled", ""));
			}
			attributes
		})
		.collect();
	let rows: Vec<(&str, Vec<Piece>)> = rooms
		.iter()
		.zip(notes.iter().zip(&buttons))
		.map(|(room, (note, button))| {
			let pieces = vec![
				Piece {
					tag: "span",
					text: &room.title,
					attributes: &[("class", "name")],
				},
				Piece {
					tag: "span",
					text: note,
					attributes: &[("class", "note")],
				},
				Piece {
					tag: "button",
					text: "Join Room",
					attributes: button,
				},
			];
			("", pieces)
		})
		.collect();
	page::set_rows("room-list", "li", &rows);
}

fn show_lobby(game: &Game) {
	let Some(session) = &game.session else {
		page::set_text("lobby-title", "Lobby");
		page::set_text("lobby-note", if game.joining { "Joining the room…" } else { "" });
		page::set_rows("players", "li", &[]);
		for id in ["bots", "rename", "start"] {
			page::set_hidden(id, true);
		}
		return;
	};
	let (host, room) = (session.is_host(), session.room());
	page::set_text("lobby-title", if game.online { &room.title } else { "Solo Flight" });
	let note = if !game.online {
		"Choose the bots, and start the match.".to_owned()
	} else if host {
		"You host this room. Choose the bots, and start the match when the players are here.".to_owned()
	} else if session.phase() == Phase::Match {
		"A match runs. You join it now…".to_owned()
	} else {
		format!(
			"{} hosts this room, and starts the match.",
			session.name_of(PlaneId::Player(room.owner))
		)
	};
	page::set_text("lobby-note", &note);

	let players = session.players();
	let numbers: Vec<String> = players.iter().map(|(number, _)| number.to_string()).collect();
	let labels: Vec<String> = players
		.iter()
		.map(|(number, name)| match (*number == room.you, *number == room.owner) {
			(true, true) => format!("{name} (you, host)"),
			(true, false) => format!("{name} (you)"),
			(false, true) => format!("{name} (host)"),
			(false, false) => name.clone(),
		})
		.collect();
	let buttons: Vec<[(&str, &str); 4]> = numbers
		.iter()
		.map(|number| {
			[
				("type", "button"),
				("data-action", "kick"),
				("data-value", number.as_str()),
				("class", "quiet"),
			]
		})
		.collect();
	let rows: Vec<(&str, Vec<Piece>)> = players
		.iter()
		.enumerate()
		.map(|(index, (number, _))| {
			let mut pieces = vec![Piece {
				tag: "span",
				text: &labels[index],
				attributes: &[("class", "name")],
			}];
			if host && game.online && *number != room.you {
				let text = if game.is_armed(&format!("kick {number}")) {
					"Confirm Removal"
				} else {
					"Remove Player"
				};
				pieces.push(Piece {
					tag: "button",
					text,
					attributes: &buttons[index],
				});
			}
			("", pieces)
		})
		.collect();
	page::set_rows("players", "li", if game.online { &rows } else { &[] });

	let settings = session.settings();
	page::set_hidden("bots", false);
	page::set_text("bots-count", &settings.bots.to_string());
	page::set_field("skill", settings.skill.name());
	page::set_disabled("skill", !host);
	page::set_disabled("bots-less", !host || settings.bots == 0);
	page::set_disabled("bots-more", !host || settings.bots >= MAX_BOTS);
	page::set_hidden("rename", !(host && game.online));
	page::set_hidden("start", !host);
	debug_assert!(
		Skill::from_name(settings.skill.name()).is_some(),
		"the page lists each skill by its name"
	);
}

fn show_settings(game: &Game) {
	let indexes: Vec<String> = (0..Bind::ALL.len()).map(|index| index.to_string()).collect();
	let keys: Vec<String> = Bind::ALL
		.iter()
		.map(|&bind| {
			if game.binding == Some(bind) {
				"Press a Key…".to_owned()
			} else {
				key_name(game.options.key(bind))
			}
		})
		.collect();
	let buttons: Vec<[(&str, &str); 4]> = indexes
		.iter()
		.map(|index| {
			[
				("type", "button"),
				("data-action", "bind"),
				("data-value", index.as_str()),
				("class", "quiet"),
			]
		})
		.collect();
	let rows: Vec<(&str, Vec<Piece>)> = Bind::ALL
		.iter()
		.enumerate()
		.map(|(index, bind)| {
			let label = Piece {
				tag: "th",
				text: bind.label(),
				attributes: &[("scope", "row")],
			};
			(
				"",
				vec![
					label,
					Piece {
						tag: "button",
						text: &keys[index],
						attributes: &buttons[index],
					},
				],
			)
		})
		.collect();
	page::set_rows("binding-rows", "tr", &rows);
	let note = match game.binding {
		Some(bind) => format!(
			"Press the new key for “{}”. The pause key leaves the old key.",
			bind.label()
		),
		None => String::new(),
	};
	page::set_text("bind-note", &note);
	if game.binding.is_none() {
		page::set_field("sensitivity", &game.options.sensitivity.to_string());
		page::set_field("invert", &game.options.invert.to_string());
	}
}

/// Move the element with this id to a place of the screen, in CSS pixels, with a scale.
fn move_to(id: &str, place: Vec2, scale: f32) {
	page::set_style(
		id,
		"transform",
		&format!(
			"translate({:.1}px,{:.1}px) translate(-50%,-14px) scale({scale:.2})",
			place.x, place.y
		),
	);
}

fn show_display(
	game: Res<Game>,
	time: Res<Time>,
	camera: Single<(&Camera, &Transform), With<Camera3d>>,
	mut feed: Local<Vec<(String, f32)>>,
	mut hit: Local<f32>,
) {
	let running = game
		.session
		.as_ref()
		.and_then(Session::game)
		.filter(|_| matches!(game.screen, Screen::Playing | Screen::Paused));
	let (Some(session), Some(running)) = (&game.session, running) else {
		feed.clear();
		return page::set_hidden("hud", true);
	};
	page::set_hidden("hud", false);
	let me = running.me();
	let planes = running.planes();
	let Some(own) = running.plane(me) else { return };
	let (camera, camera_place) = camera.into_inner();
	let camera_frame = GlobalTransform::from(*camera_place);
	let size = camera.logical_viewport_size().unwrap_or(Vec2::new(1280.0, 720.0));
	let on_screen = |place: Vec3| {
		camera
			.world_to_viewport(&camera_frame, place)
			.ok()
			.filter(|at| at.x >= 0.0 && at.y >= 0.0 && at.x <= size.x && at.y <= size.y)
	};

	// The parts and the flight.
	for part in Part::ALL {
		let (id, health) = (format!("bar-{}", part as usize), own.health(part) / part.full_health());
		page::set_style(&id, "--health", &format!("{health:.2}"));
		page::set_class(&id, "low", health > 0.0 && health < 0.5);
		page::set_class(&id, "gone", health <= 0.0);
	}
	page::set_text("speed", &format!("{:.0} m/s", own.speed));
	page::set_text("height", &format!("{:.0} m", own.position.y));
	page::set_text("throttle", &format!("{:.0} %", game.throttle * 100.0));
	page::set_text("boost", &format!("{:.0} %", own.boost() * 100.0));
	page::set_text("missiles", &running.missiles_ready(me).unwrap_or(0).to_string());

	// The gun sight, which shows each hit of the player for a moment.
	*hit = (*hit - time.delta_secs()).max(0.0);
	if game
		.events
		.iter()
		.any(|event| matches!(event, Event::Hit { by, .. } if *by == me))
	{
		*hit = HIT_SECONDS;
	}
	let alive = running.respawn_in(me).is_none();
	let sight = on_screen(own.position + own.forward() * SIGHT_DISTANCE).filter(|_| alive);
	page::set_hidden("nose", sight.is_none());
	if let Some(place) = sight {
		page::set_style(
			"nose",
			"transform",
			&format!("translate({:.1}px,{:.1}px)", place.x, place.y),
		);
	}
	page::set_class("nose", "hit", *hit > 0.0);

	// The box of the missile lock, which closes on the target.
	let lock = running.lock(me).and_then(|(target, closed)| {
		let state = planes.iter().find(|(id, _)| *id == target)?.1;
		Some((on_screen(state.position)?, closed))
	});
	page::set_hidden("lock-box", lock.is_none());
	if let Some((place, closed)) = lock {
		page::set_style(
			"lock-box",
			"transform",
			&format!(
				"translate({:.1}px,{:.1}px) scale({:.2})",
				place.x,
				place.y,
				2.0 - closed
			),
		);
		page::set_class("lock-box", "ready", closed >= 1.0);
	}

	// A marker on each other plane. A plane outside the picture has its marker at the edge.
	let others = planes.iter().filter(|(id, state)| *id != me && state.alive);
	let mut used = 0;
	for (id, state) in others.take(MARKERS) {
		let marker = format!("marker-{used}");
		used += 1;
		let distance = state.position.distance(own.position);
		let seen = on_screen(state.position);
		let place = seen.unwrap_or_else(|| {
			let local = camera_place.rotation.inverse() * (state.position - camera_place.translation);
			let toward = Vec2::new(local.x, -local.y).normalize_or(Vec2::Y);
			size / 2.0 + toward * (size.min_element() / 2.0 - 40.0)
		});
		page::set_hidden(&marker, false);
		page::set_text(&marker, &format!("{} {distance:.0} m", session.name_of(*id)));
		move_to(&marker, place, 1.0);
		page::set_class(&marker, "safe", seen.is_some() && state.protected);
		page::set_class(&marker, "edge", seen.is_none());
	}
	for index in used..MARKERS {
		page::set_hidden(&format!("marker-{index}"), true);
	}

	// The most urgent warning.
	let missile = running
		.missiles()
		.iter()
		.filter(|missile| missile.target == me && missile.is_seeking())
		.map(|missile| missile.position.distance(own.position));
	let missile = missile.min_by(f32::total_cmp);
	let locked = planes.iter().any(|(_, state)| state.alive && state.lock == Some(me));
	let outside = Vec2::new(own.position.x, own.position.z).length() > tuning::ARENA_RADIUS - 150.0;
	let alert = if let Some(wait) = running.respawn_in(me) {
		format!("Your plane is destroyed.\nYou fly again in {wait:.1} s.")
	} else if let Some(distance) = missile {
		format!("MISSILE {distance:.0} m\nBreak hard across its path!")
	} else if locked {
		"An enemy locks a missile on you.\nBreak away!".to_owned()
	} else if outside {
		"The edge of the arena.\nYour plane turns back.".to_owned()
	} else if own.is_protected() {
		"Protected after the spawn.".to_owned()
	} else {
		String::new()
	};
	page::set_text("alert", &alert);

	// The kills, in the order in which every machine sees them.
	for event in &game.events {
		if let Event::Kill { victim, killer } = event {
			let line = match killer {
				Some(killer) => format!("{} destroyed {}", session.name_of(*killer), session.name_of(*victim)),
				None => format!("{} crashed", session.name_of(*victim)),
			};
			feed.push((line, FEED_SECONDS));
		}
	}
	feed.iter_mut().for_each(|(_, left)| *left -= time.delta_secs());
	feed.retain(|(_, left)| *left > 0.0);
	let lines: Vec<(&str, Vec<Piece>)> = feed
		.iter()
		.rev()
		.take(5)
		.rev()
		.map(|(line, _)| {
			(
				"",
				vec![Piece {
					tag: "span",
					text: line,
					attributes: &[],
				}],
			)
		})
		.collect();
	page::set_rows("feed", "li", &lines);

	let scores = session.scores();
	let numbers: Vec<String> = scores
		.iter()
		.map(|score| format!("{} / {}", score.kills, score.deaths))
		.collect();
	let rows: Vec<(&str, Vec<Piece>)> = scores
		.iter()
		.zip(&numbers)
		.take(8)
		.map(|(score, numbers)| {
			let cell = |text| Piece {
				tag: "td",
				text,
				attributes: &[],
			};
			(
				if score.plane == me { "me" } else { "" },
				vec![cell(score.name.as_str()), cell(numbers)],
			)
		})
		.collect();
	page::set_rows("score-rows", "tr", &rows);
}

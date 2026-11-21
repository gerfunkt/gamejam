//! What the page shows around the sky: the menus, and the display of a match.

use bevy::prelude::*;
use sim::{
	Event, Part, Phase, Session, Skill,
	room::MAX_MEMBERS,
	tuning,
	wire::{MAX_BOTS, PlaneId, PlaneState},
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
/// How long the praise for a kill shows, in seconds.
const PRAISE_SECONDS: f32 = 2.5;
/// The distance from the edge of the picture at which the marker of a plane outside it shows, in CSS pixels.
const EDGE_INSET: f32 = 46.0;
/// The widest angle off the nose at which a plane without a lock on it becomes the target, in radians.
const TARGET_CONE: f32 = 0.5;
/// The range of the radar, in metres.
const RADAR_RANGE: f32 = 1500.0;
/// The height above the ground below which the display tells the pilot to pull up, in metres.
const LOW_HEIGHT: f32 = 60.0;

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
	let target_bars: Vec<String> = (0..Part::ALL.len())
		.map(|index| format!("target-bar-{index}"))
		.collect();
	let target_attributes: Vec<[(&str, &str); 2]> = target_bars
		.iter()
		.map(|id| [("class", "bar"), ("id", id.as_str())])
		.collect();
	let rows: Vec<(&str, Vec<Piece>)> = PART_NAMES
		.iter()
		.zip(&target_attributes)
		.map(|(name, attributes)| {
			(
				"",
				vec![
					Piece {
						tag: "dt",
						text: name,
						attributes: &[],
					},
					Piece {
						tag: "dd",
						text: "",
						attributes,
					},
				],
			)
		})
		.collect();
	page::set_rows("target-parts", "div", &rows);
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
		page::set_field("calm", &game.options.calm.to_string());
		page::set_field("effects-volume", &game.options.effects_volume.to_string());
		page::set_field("voice-volume", &game.options.voice_volume.to_string());
	}
}

/// Move the element with this id to a place of the screen, in CSS pixels.
fn move_to(id: &str, place: Vec2) {
	page::set_style(
		id,
		"transform",
		&format!("translate({:.1}px,{:.1}px) translate(-50%,-14px)", place.x, place.y),
	);
}

/// Show the state of a part on the bar with this id: how full, and whether low or gone.
fn show_health(id: &str, health: f32) {
	page::set_style(id, "--health", &format!("{health:.2}"));
	page::set_class(id, "low", health > 0.0 && health < 0.5);
	page::set_class(id, "gone", health <= 0.0);
}

/// Return `count` filled marks out of `most`, such as "●●○".
fn pips(count: u8, most: u8, full: char) -> String {
	(0..most).map(|index| if index < count { full } else { '○' }).collect()
}

/// Return the heading of a nose as degrees east of north, with the point of the compass.
fn heading(nose: Vec3) -> String {
	let degrees = (nose.x.atan2(-nose.z).to_degrees() + 360.0) % 360.0;
	let points = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
	format!(
		"{degrees:03.0}° {}",
		points[((degrees + 22.5) / 45.0) as usize % points.len()]
	)
}

/// What the display remembers between frames.
#[derive(Default)]
struct Memory {
	/// The lines of the kill feed, each with the seconds for which it still shows.
	feed: Vec<(String, f32)>,
	/// The seconds for which the gun sight still shows a hit.
	hit: f32,
	/// The seconds for which the edge of the picture still shows a hit on the own plane.
	hurt: f32,
	/// The praise for a kill, and the seconds for which it still shows.
	praise: (String, f32),
}

/// Draw the radar: the planes around the player from above, with the nose of the player at the top.
fn draw_radar(own: &PlaneState, me: PlaneId, planes: &[(PlaneId, PlaneState)], missiles: &[sim::Missile]) {
	let Some(context) = page::canvas("radar") else { return };
	let (size, range) = (176.0_f64, f64::from(RADAR_RANGE));
	let (centre, radius) = (size / 2.0, size / 2.0 - 4.0);
	context.clear_rect(0.0, 0.0, size, size);
	context.set_fill_style_str("rgba(13, 18, 26, 0.55)");
	context.set_stroke_style_str("#2c3a4d");
	context.set_line_width(1.0);
	for (part, fill) in [(1.0, true), (0.5, false)] {
		context.begin_path();
		let _ = context.arc(centre, centre, radius * part, 0.0, std::f64::consts::TAU);
		if fill {
			context.fill();
		}
		context.stroke();
	}
	// The directions on the ground: ahead of the nose, and to its right.
	let nose = own.forward();
	let ahead = Vec2::new(nose.x, nose.z).normalize_or(Vec2::NEG_Y);
	let right = Vec2::new(-ahead.y, ahead.x);
	let dot = |place: Vec3, colour: &str, size: f64| {
		let offset = Vec2::new(place.x - own.position.x, place.z - own.position.z);
		let across = f64::from(offset.dot(right)) / range;
		let along = f64::from(offset.dot(ahead)) / range;
		// A plane beyond the range shows at the rim.
		let far = (across * across + along * along).sqrt().max(1.0);
		context.set_fill_style_str(colour);
		context.begin_path();
		let _ = context.arc(
			centre + across / far * radius,
			centre - along / far * radius,
			size,
			0.0,
			std::f64::consts::TAU,
		);
		context.fill();
	};
	for (id, state) in planes.iter().filter(|(id, state)| *id != me && state.alive) {
		let colour = match (state.protected, state.lock == Some(me), id) {
			(true, ..) => "#93a3b8",
			(false, true, _) => "#ffffff",
			(false, false, PlaneId::Player(_)) => "#ff6b57",
			(false, false, PlaneId::Bot(_)) => "#e0895e",
		};
		dot(state.position, colour, 3.5);
	}
	for missile in missiles
		.iter()
		.filter(|missile| missile.target == me && missile.is_seeking() && !missile.is_lured())
	{
		dot(missile.position, "#f2c230", 2.5);
	}
	// The player, as a mark that points up.
	context.set_fill_style_str("#6fd08c");
	context.begin_path();
	context.move_to(centre, centre - 7.0);
	context.line_to(centre + 5.0, centre + 5.0);
	context.line_to(centre - 5.0, centre + 5.0);
	context.close_path();
	context.fill();
}

fn show_display(
	game: Res<Game>,
	time: Res<Time>,
	camera: Single<(&Camera, &Transform), With<Camera3d>>,
	mut memory: Local<Memory>,
) {
	let running = game
		.session
		.as_ref()
		.and_then(Session::game)
		.filter(|_| matches!(game.screen, Screen::Playing | Screen::Paused));
	let (Some(session), Some(running)) = (&game.session, running) else {
		*memory = Memory::default();
		return page::set_hidden("hud", true);
	};
	page::set_hidden("hud", false);
	let me = running.me();
	let planes = running.planes();
	let (Some(own), Some(own_state)) = (
		running.plane(me),
		planes.iter().find(|(id, _)| *id == me).map(|(_, state)| *state),
	) else {
		return;
	};
	let alive = running.respawn_in(me).is_none();
	let dt = time.delta_secs();
	let (camera, camera_place) = camera.into_inner();
	let camera_frame = GlobalTransform::from(*camera_place);
	let size = camera.logical_viewport_size().unwrap_or(Vec2::new(1280.0, 720.0));
	let on_screen = |place: Vec3| {
		camera
			.world_to_viewport(&camera_frame, place)
			.ok()
			.filter(|at| at.x >= 0.0 && at.y >= 0.0 && at.x <= size.x && at.y <= size.y)
	};

	// The own plane: its parts as bars and as a picture, its flight, and its weapons.
	for part in Part::ALL {
		let health = own.health(part) / part.full_health();
		show_health(&format!("bar-{}", part as usize), health);
		let shape = format!("shape-{}", part as usize);
		page::set_class(&shape, "low", health > 0.0 && health < 0.5);
		page::set_class(&shape, "gone", health <= 0.0);
	}
	let above_ground = own.position.y - sim::arena::ground_height(own.position.x, own.position.z);
	page::set_text("speed", &format!("{:.0} m/s", own.speed));
	page::set_text("height", &format!("{above_ground:.0} m"));
	page::set_text("heading", &heading(own.forward()));
	page::set_style("throttle", "--health", &format!("{:.2}", game.throttle));
	page::set_style("boost", "--health", &format!("{:.2}", own.boost()));
	page::set_text(
		"missiles",
		&pips(running.missiles_ready(me).unwrap_or(0), tuning::MISSILES, '●'),
	);
	page::set_text(
		"flares",
		&pips(running.flares_ready(me).unwrap_or(0), tuning::FLARES, '◆'),
	);

	// The events of this frame: a hit by the player, a hit on the player, and a kill.
	(memory.hit, memory.hurt, memory.praise.1) = (
		(memory.hit - dt).max(0.0),
		(memory.hurt - dt).max(0.0),
		(memory.praise.1 - dt).max(0.0),
	);
	for event in &game.events {
		match event {
			Event::Hit { by, .. } if *by == me => memory.hit = HIT_SECONDS,
			Event::Damage { plane, .. } if *plane == me => memory.hurt = HIT_SECONDS,
			Event::Kill { victim, killer } => {
				let line = match killer {
					Some(killer) => format!("{} destroyed {}", session.name_of(*killer), session.name_of(*victim)),
					None => format!("{} crashed", session.name_of(*victim)),
				};
				memory.feed.push((line, FEED_SECONDS));
				if *killer == Some(me) {
					memory.praise = (format!("You destroyed {}", session.name_of(*victim)), PRAISE_SECONDS);
				}
			}
			_ => {}
		}
	}
	page::set_class("hud", "hurt", memory.hurt > 0.0 && !game.options.calm);
	page::set_text("praise", if memory.praise.1 > 0.0 { &memory.praise.0 } else { "" });

	// The gun sight, which shows each hit of the player for a moment.
	let sight = on_screen(own.position + own.forward() * SIGHT_DISTANCE).filter(|_| alive);
	page::set_hidden("nose", sight.is_none());
	if let Some(place) = sight {
		page::set_style(
			"nose",
			"transform",
			&format!("translate({:.1}px,{:.1}px)", place.x, place.y),
		);
	}
	page::set_class("nose", "hit", memory.hit > 0.0 && !game.options.calm);

	// The box of the missile lock, which closes on the target.
	let lock = running
		.lock(me)
		.and_then(|(target, closed)| Some((target, planes.iter().find(|(id, _)| *id == target)?.1, closed)));
	let lock_place = lock.and_then(|(_, state, closed)| Some((on_screen(state.position)?, closed)));
	page::set_hidden("lock-box", lock_place.is_none());
	if let Some((place, closed)) = lock_place {
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
	let others: Vec<&(PlaneId, PlaneState)> = planes.iter().filter(|(id, state)| *id != me && state.alive).collect();
	for (index, (id, state)) in others.iter().take(MARKERS).enumerate() {
		let marker = format!("marker-{index}");
		let seen = on_screen(state.position);
		let place = seen.unwrap_or_else(|| {
			// The direction of the plane across the picture, stretched until it meets the edge.
			let local = camera_place.rotation.inverse() * (state.position - camera_place.translation);
			let toward = Vec2::new(local.x, -local.y).normalize_or(Vec2::Y);
			let reach = (size / 2.0 - EDGE_INSET) / toward.abs().max(Vec2::splat(1e-3));
			size / 2.0 + toward * reach.min_element()
		});
		page::set_hidden(&marker, false);
		page::set_text(
			&marker,
			&format!(
				"{} {:.0} m",
				session.name_of(*id),
				state.position.distance(own.position)
			),
		);
		move_to(&marker, place);
		page::set_class(&marker, "safe", seen.is_some() && state.protected);
		page::set_class(&marker, "edge", seen.is_none());
	}
	for index in others.len().min(MARKERS)..MARKERS {
		page::set_hidden(&format!("marker-{index}"), true);
	}

	// The target: the plane of the lock, or else the plane nearest to the nose.
	let ahead = others
		.iter()
		.filter(|(_, state)| own.forward().angle_between(state.position - own.position) < TARGET_CONE);
	let nearest = ahead.min_by(|a, b| {
		a.1.position
			.distance(own.position)
			.total_cmp(&b.1.position.distance(own.position))
	});
	let target = lock
		.map(|(id, state, _)| (id, state))
		.or(nearest.map(|(id, state)| (*id, *state)));
	page::set_hidden("target", target.is_none());
	if let Some((id, state)) = target {
		let locked = lock.is_some_and(|(_, _, closed)| closed >= 1.0);
		let distance = state.position.distance(own.position);
		page::set_text(
			"target-name",
			&format!(
				"{}{} · {distance:.0} m",
				if locked { "LOCKED · " } else { "" },
				session.name_of(id)
			),
		);
		for part in Part::ALL {
			show_health(
				&format!("target-bar-{}", part as usize),
				f32::from(state.health[part as usize]) / part.full_health(),
			);
		}
	}

	// The most urgent warning.
	let missile = running
		.missiles()
		.iter()
		.filter(|missile| missile.target == me && missile.is_seeking() && !missile.is_lured());
	let missile = missile
		.map(|missile| missile.position.distance(own.position))
		.min_by(f32::total_cmp);
	let locked = planes.iter().any(|(_, state)| state.alive && state.lock == Some(me));
	let outside = Vec2::new(own.position.x, own.position.z).length() > tuning::ARENA_RADIUS - 150.0;
	let alert = if let Some(wait) = running.respawn_in(me) {
		format!("Your plane is destroyed.\nYou fly again in {wait:.1} s.")
	} else if own.is_falling() {
		"You lost a wing.\nThe plane falls.".to_owned()
	} else if let Some(distance) = missile {
		format!("MISSILE {distance:.0} m\nRelease a flare, or break hard across its path!")
	} else if locked {
		"An enemy locks a missile on you.\nBreak away!".to_owned()
	} else if above_ground < LOW_HEIGHT {
		"PULL UP".to_owned()
	} else if outside {
		"The edge of the arena.\nYour plane turns back.".to_owned()
	} else if own.is_protected() {
		"Protected after the spawn.".to_owned()
	} else {
		String::new()
	};
	page::set_text("alert", &alert);

	// The kills, in the order in which every machine sees them, and the scores.
	memory.feed.iter_mut().for_each(|(_, left)| *left -= dt);
	memory.feed.retain(|(_, left)| *left > 0.0);
	let lines: Vec<(&str, Vec<Piece>)> = memory
		.feed
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

	draw_radar(&own_state, me, &planes, running.missiles());
}

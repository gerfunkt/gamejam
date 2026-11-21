//! The flow of the client: its screens, its session, and the steps of a match.

use bevy::prelude::*;
use sim::{
	Controls, Event, FixedSteps, Phase, Session, Skill, bot_controls,
	room::{CloseCode, Join, RoomEntry},
	tuning,
	wire::{MAX_BOTS, PlaneId, Settings},
};

use crate::{
	audio,
	input::{self, Bind, Lens, Options},
	net::{self, Incoming},
	page::{self, UiEvent},
};

/// How long a control that needs a second click waits for it, in milliseconds.
const CONFIRM_TIME: f64 = 4000.0;
/// The frames for which the page shows that a match starts, before the match starts.
const START_NOTICE_FRAMES: u8 = 2;
/// The time between two lines of numbers for the browser tests, in seconds.
const STAT_INTERVAL: f32 = 2.0;

/// What the page shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
	Menu,
	Rooms,
	Lobby,
	Settings,
	Help,
	Playing,
	Paused,
	Results,
	Lost,
}

/// The numbers of a match that the browser tests read from the console.
#[derive(Default)]
struct Stats {
	since_line: f32,
	frames: u32,
	kills: u32,
	hits_sent: u32,
	hits_taken: u32,
	bursts: u32,
}

/// The state of the client outside the engine.
#[derive(Resource)]
pub struct Game {
	pub screen: Screen,
	/// The screen to which the settings return.
	back: Screen,
	/// The session in the room of the player. A solo game is a session of its own.
	pub session: Option<Session>,
	/// Whether the session talks to the bus.
	pub online: bool,
	/// Whether the client waits for the first status of a room.
	pub joining: bool,
	pub options: Options,
	/// The setting of the engine of the plane, from 0 to 1.
	pub throttle: f32,
	steps: FixedSteps,
	/// The camera, as the aim of the mouse needs it. The view updates it.
	pub lens: Lens,
	/// What happened in the match in this frame, for the view and the display.
	pub events: Vec<Event>,
	/// The room list of the server, or the reason why the client has none.
	pub rooms: Option<Result<Vec<RoomEntry>, String>>,
	/// The action whose key the player changes now.
	pub binding: Option<Bind>,
	/// Why the connection ended, for the player.
	pub lost: String,
	/// The control that waits for its second click, and until when.
	armed: Option<(String, f64)>,
	/// The frames until the match starts, after the host chose to start it.
	starting: Option<u8>,
	/// A test hook: a bot flies the plane of the player.
	bot_pilot: bool,
	/// A test hook: the client writes its numbers to the console.
	log_stats: bool,
	stats: Stats,
}

impl Game {
	fn new() -> Self {
		Self {
			screen: Screen::Menu,
			back: Screen::Menu,
			session: None,
			online: false,
			joining: false,
			options: Options::load(),
			throttle: 0.6,
			steps: FixedSteps::default(),
			lens: Lens {
				frame: Quat::IDENTITY,
				half_height: 0.58,
				aspect: 1.78,
			},
			events: Vec::new(),
			rooms: None,
			binding: None,
			lost: String::new(),
			armed: None,
			starting: None,
			bot_pilot: page::query("pilot").as_deref() == Some("bot"),
			log_stats: page::query("stat").as_deref() == Some("1"),
			stats: Stats::default(),
		}
	}

	/// Return whether the host chose to start the match, and the match did not start yet.
	pub fn is_starting(&self) -> bool {
		self.starting.is_some()
	}

	/// Return how far along the current frame is between two steps of the rules, from 0 to 1.
	pub fn step_fraction(&self) -> f32 {
		self.steps.fraction()
	}

	/// Return whether the control with this key waits for its second click.
	pub fn is_armed(&self, key: &str) -> bool {
		self.armed
			.as_ref()
			.is_some_and(|(armed, until)| armed == key && page::now() < *until)
	}

	/// Return whether the player confirmed the control with this key by a second click.
	fn confirmed(&mut self, key: &str, now: f64) -> bool {
		if self.is_armed(key) {
			self.armed = None;
			return true;
		}
		self.armed = Some((key.to_owned(), now + CONFIRM_TIME));
		false
	}

	/// Open the pause menu of a match, or close it.
	fn toggle_pause(&mut self) {
		self.screen = match self.screen {
			Screen::Playing => Screen::Paused,
			Screen::Paused => Screen::Playing,
			other => other,
		};
	}

	fn saved_settings(&self) -> Settings {
		Settings {
			bots: self.options.bots.min(MAX_BOTS),
			skill: self.options.skill,
		}
	}

	fn enter(&mut self, join: &Join) {
		net::connect(join);
		(self.session, self.online, self.joining, self.screen) = (None, true, true, Screen::Lobby);
	}

	fn leave(&mut self) {
		net::disconnect();
		(self.session, self.joining, self.screen) = (None, false, Screen::Menu);
	}

	/// Change the bots of the lobby, and keep the choice for the next visit.
	fn choose(&mut self, change: impl FnOnce(&mut Settings)) {
		let Some(session) = self
			.session
			.as_mut()
			.filter(|session| session.is_host() && session.phase() != Phase::Match)
		else {
			return;
		};
		let mut settings = session.settings();
		change(&mut settings);
		session.set_settings(settings);
		(self.options.bots, self.options.skill) = (session.settings().bots, session.settings().skill);
		self.options.save();
	}

	fn act(&mut self, action: &str, value: &str, now: f64) {
		match action {
			"solo" => {
				let mut session = Session::solo(&self.options.name);
				session.set_settings(self.saved_settings());
				(self.session, self.online, self.screen) = (Some(session), false, Screen::Lobby);
			}
			"online" => {
				(self.rooms, self.screen) = (None, Screen::Rooms);
				net::request_rooms();
			}
			"refresh" => net::request_rooms(),
			"join" => self.enter(&Join::Room(value.to_owned())),
			"create" => self.enter(&Join::Create(page::field("room-title"))),
			"bots-less" => self.choose(|settings| settings.bots = settings.bots.saturating_sub(1)),
			"bots-more" => self.choose(|settings| settings.bots += 1),
			// The first frames of a match can take long, so the page says that the match starts before it does.
			"start" => self.starting = Some(START_NOTICE_FRAMES),
			"rename" => {
				if let Some(session) = &mut self.session {
					session.rename_room(&page::field("new-title"));
				}
			}
			"kick" => {
				if let (true, Ok(number), Some(session)) = (
					self.confirmed(&format!("kick {value}"), now),
					value.parse(),
					&mut self.session,
				) {
					session.remove_member(number);
				}
			}
			"leave" => {
				let in_match = self
					.session
					.as_ref()
					.is_some_and(|session| session.phase() == Phase::Match);
				if !in_match || self.confirmed("leave", now) {
					self.leave();
				}
			}
			"menu" => self.leave(),
			"settings" => (self.back, self.screen) = (self.screen, Screen::Settings),
			"help" => (self.back, self.screen) = (self.screen, Screen::Help),
			"back" => (self.binding, self.screen) = (None, self.back),
			"bind" => self.binding = Bind::from_index(value),
			"defaults" => {
				self.options.reset_keys();
				self.options.save();
			}
			"resume" => self.screen = Screen::Playing,
			"end" => {
				if let (true, Some(session)) = (self.confirmed("end", now), &mut self.session) {
					session.request_end();
				}
			}
			"close-results" => {
				if let Some(session) = &mut self.session {
					session.close_results();
				}
				self.screen = Screen::Lobby;
			}
			_ => {}
		}
	}

	fn change(&mut self, id: &str, value: &str) {
		match id {
			"name" => self.options.name = sim::wire::clean_name(value),
			"sensitivity" => self.options.sensitivity = value.parse().unwrap_or(1.0),
			"invert" => self.options.invert = value == "true",
			"calm" => self.options.calm = value == "true",
			"effects-volume" => self.options.effects_volume = value.parse().unwrap_or(0.7),
			"voice-volume" => self.options.voice_volume = value.parse().unwrap_or(0.7),
			"skill" => {
				if let Some(skill) = Skill::from_name(value) {
					self.choose(|settings| settings.skill = skill);
				}
			}
			_ => return,
		}
		self.options.save();
	}

	/// Return the controls of this step: those of the player, of the test bot, or none.
	fn controls(&mut self, me: PlaneId, dt: f32) -> Option<Controls> {
		let game = self.session.as_ref()?.game()?;
		if self.bot_pilot {
			let planes = game.planes();
			let state = planes.iter().find(|(id, _)| *id == me)?.1;
			return Some(bot_controls(
				me,
				&state,
				&planes,
				game.missiles(),
				game.power_ups(),
				Skill::Ace,
			));
		}
		let plane = game.plane(me)?;
		if self.screen == Screen::Playing {
			return Some(input::read(&self.options, self.lens, &mut self.throttle, dt));
		}
		// A menu is open over a match that runs on: the plane holds its course.
		Some(Controls {
			aim: plane.forward(),
			throttle: self.throttle,
			boost: false,
			fire: false,
			launch: false,
			flare: false,
		})
	}
}

pub fn plugin(app: &mut App) {
	page::watch_menus();
	page::watch_fullscreen();
	input::watch();
	page::set_field("name", &Options::load().name);
	app.insert_resource(Game::new()).add_systems(
		Update,
		(handle_page, handle_bus, simulate, send, log_stats)
			.chain()
			.in_set(Flow),
	);
}

/// The systems that change the state of the client in a frame. The view and the display run after them.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Flow;

fn handle_page(mut game: ResMut<Game>) {
	let game = &mut *game;
	let now = page::now();
	game.events.clear();
	// The clicks come first: a click on a key in the settings must count before the key that follows it.
	for event in page::take_ui_events() {
		match event {
			UiEvent::Action { action, value } => {
				// A browser allows sound only after the player acted, and a click is such an act.
				audio::start();
				audio::play("ui_click", 0.5);
				game.act(&action, &value, now);
			}
			UiEvent::Change { id, value } => game.change(&id, &value),
		}
	}
	match game.starting {
		Some(0) => {
			game.starting = None;
			if let Some(session) = &mut game.session {
				session.start_match((js_sys::Math::random() * f64::from(u32::MAX)) as u32);
			}
		}
		Some(frames) => game.starting = Some(frames - 1),
		None => {}
	}
	if input::take_pause() {
		game.toggle_pause();
	}
	for code in input::take_pressed() {
		if let Some(bind) = game.binding.take() {
			// The pause key leaves the choice of a key, so it cannot be given to another action here.
			if code != game.options.key(Bind::Pause) {
				game.options.set_key(bind, &code);
				game.options.save();
			}
		} else if code == game.options.key(Bind::Pause) {
			game.toggle_pause();
		}
	}
	if input::take_left_page() && game.screen == Screen::Playing && !game.bot_pilot {
		game.screen = Screen::Paused;
	}
}

fn handle_bus(mut game: ResMut<Game>) {
	let game = &mut *game;
	if let Some(rooms) = net::take_rooms() {
		game.rooms = Some(rooms);
	}
	for incoming in net::take_incoming() {
		match incoming {
			Incoming::Status(status) => match &mut game.session {
				Some(session) => session.room_changed(status),
				None => {
					let mut session = Session::new(&game.options.name, status);
					session.set_settings(game.saved_settings());
					(game.session, game.joining) = (Some(session), false);
				}
			},
			Incoming::Message { sender, message, at } => {
				if let Some(session) = &mut game.session {
					session.receive(at, sender, &message);
				}
			}
			Incoming::Closed { code } => {
				game.lost = match CloseCode::from_code(code) {
					Some(CloseCode::Removed) => "The host removed you from the room.",
					Some(CloseCode::NoSuchRoom) => "That room no longer exists. Choose another room, or create one.",
					Some(CloseCode::RoomFull) => "That room is full. Choose another room, or create one.",
					Some(CloseCode::TooManyRooms) => {
						"The server holds no more rooms. Join one of the rooms in the list."
					}
					Some(CloseCode::FellBehind) => {
						"This machine fell too far behind the room. Check the network, and join the room again."
					}
					None => "The connection to the server ended. Check the network, and join the room again.",
				}
				.to_owned();
				(game.session, game.joining, game.screen) = (None, false, Screen::Lost);
			}
		}
	}
}

fn simulate(mut game: ResMut<Game>, time: Res<Time>) {
	let game = &mut *game;
	let Some(session) = &game.session else {
		input::set_in_flight(false);
		return;
	};
	game.screen = match (session.phase(), game.screen) {
		(Phase::Match, Screen::Lobby | Screen::Results) => Screen::Playing,
		(Phase::Results, Screen::Playing | Screen::Paused | Screen::Lobby) => Screen::Results,
		(Phase::Lobby, Screen::Playing | Screen::Paused | Screen::Results) => Screen::Lobby,
		(_, screen) => screen,
	};
	input::set_in_flight(game.screen == Screen::Playing);
	let Some(me) = session.game().map(sim::Match::me) else {
		return;
	};
	// A solo match waits while a menu is open. An online match runs on for everyone.
	if game.screen != Screen::Playing && !game.online {
		game.steps = FixedSteps::default();
		return;
	}
	let now = page::now();
	for _ in 0..game.steps.advance(time.delta_secs()) {
		let Some(controls) = game.controls(me, tuning::STEP) else {
			break;
		};
		let Some(session) = &mut game.session else { break };
		session.step(now, &controls);
		if !game.online {
			session.deliver_to_self(now);
		}
	}
	if let Some(running) = game.session.as_mut().and_then(Session::game_mut) {
		game.events = running.drain_events();
	}
}

fn send(mut game: ResMut<Game>) {
	let online = game.online;
	let Some(session) = &mut game.session else { return };
	if !online {
		session.deliver_to_self(page::now());
		return;
	}
	for message in session.drain_messages() {
		if message.is_state() {
			net::send_state(&message);
		} else {
			net::send_event(&message);
		}
	}
	for control in session.drain_controls() {
		net::send_control(&control);
	}
}

/// Write the numbers of the match to the console, where the browser tests read them.
fn log_stats(mut game: ResMut<Game>, time: Res<Time>) {
	let game = &mut *game;
	if !game.log_stats {
		return;
	}
	let me = game.session.as_ref().and_then(Session::game).map(sim::Match::me);
	for event in &game.events {
		match event {
			Event::Kill { .. } => game.stats.kills += 1,
			Event::Hit { by, .. } if Some(*by) == me => game.stats.hits_sent += 1,
			Event::Damage { plane, .. } if Some(*plane) == me => game.stats.hits_taken += 1,
			Event::Burst { .. } => game.stats.bursts += 1,
			_ => {}
		}
	}
	game.stats.frames += 1;
	game.stats.since_line += time.delta_secs();
	if game.stats.since_line < STAT_INTERVAL {
		return;
	}
	let stats = &game.stats;
	let running = game.session.as_ref().and_then(Session::game);
	let planes = running.map_or(0, |running| running.planes().len());
	let bullets = running.map_or(0, |running| running.bullets().len());
	let flares = running
		.and_then(|running| running.flares_ready(running.me()))
		.unwrap_or(0);
	let (players, host) = game
		.session
		.as_ref()
		.map_or((0, false), |session| (session.players().len(), session.is_host()));
	// The flight of the player: the place, the heading in degrees east of north, and the speed.
	let flight = running
		.and_then(|running| running.plane(running.me()))
		.map_or(String::new(), |plane| {
			let (place, nose) = (plane.position, plane.forward());
			let heading = nose.x.atan2(-nose.z).to_degrees();
			format!(
				" x={:.0} y={:.0} z={:.0} heading={heading:.0} speed={:.0}",
				place.x, place.y, place.z, plane.speed
			)
		});
	page::log(&format!(
		"STAT screen={:?} backend={} fps={:.0} planes={planes} players={players} host={host} kills={} hits_sent={} hits_taken={} bursts={} bullets={bullets} flares={flares} sounds={} throttle={:.2}{flight}",
		game.screen,
		page::backend(),
		stats.frames as f32 / stats.since_line,
		stats.kills,
		stats.hits_sent,
		stats.hits_taken,
		stats.bursts,
		audio::started(),
		game.throttle,
	));
	(game.stats.frames, game.stats.since_line) = (0, 0.0);
}

//! The session of one machine in a room: its lobby, its match, and the results.
//!
//! The host of a room is the member that the bus names as its owner. Only the host chooses the
//! bots, starts a match, and ends it. Every machine applies those choices when the bus delivers
//! them, so all machines agree. A solo game is a session in a room with one member.

use std::collections::BTreeMap;

use crate::{
	game::Match,
	plane::Controls,
	room::{Control, RoomStatus},
	wire::{MAX_BOTS, Message, PlaneId, Settings, clean_name},
};

/// What a session shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
	/// The players wait, and the host chooses the bots.
	Lobby,
	/// A match runs.
	Match,
	/// The match ended, and the player reads the results.
	Results,
}

/// One line of the results of a match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Score {
	/// The plane.
	pub plane: PlaneId,
	/// The name of its pilot.
	pub name: String,
	/// The planes that it destroyed.
	pub kills: u16,
	/// How often it was destroyed.
	pub deaths: u16,
}

/// The session of one machine in a room.
#[derive(Clone, Debug)]
pub struct Session {
	name: String,
	room: RoomStatus,
	settings: Settings,
	names: BTreeMap<u32, String>,
	game: Option<Match>,
	results: Option<Vec<Score>>,
	messages: Vec<Message>,
	controls: Vec<Control>,
	info: String,
}

impl Session {
	/// Return a session for a player with this name, in the room that `room` describes.
	///
	/// The session announces the name of its player at once.
	#[must_use]
	pub fn new(name: &str, room: RoomStatus) -> Self {
		let mut session = Self {
			name: clean_name(name),
			room: RoomStatus {
				members: Vec::new(),
				..room.clone()
			},
			settings: Settings::default(),
			names: BTreeMap::new(),
			game: None,
			results: None,
			messages: Vec::new(),
			controls: Vec::new(),
			info: String::new(),
		};
		session.room_changed(room);
		session
	}

	/// Return a session for a solo game: a room that holds only this player, who hosts it.
	#[must_use]
	pub fn solo(name: &str) -> Self {
		Self::new(
			name,
			RoomStatus {
				id: "solo".into(),
				title: "Solo".into(),
				owner: 1,
				members: vec![1],
				you: 1,
			},
		)
	}

	/// Take over a new status of the room from the bus.
	///
	/// When a member joined, the session announces its name again, and the host announces the
	/// lobby again, so that the new member learns both. A match learns who left and who hosts.
	pub fn room_changed(&mut self, room: RoomStatus) {
		let joined = room.members.iter().any(|member| !self.room.members.contains(member));
		self.room = room;
		self.names.retain(|number, _| self.room.members.contains(number));
		if joined {
			self.messages.push(Message::Profile {
				name: self.name.clone(),
			});
			if self.is_host() {
				self.announce_lobby(self.game.as_ref().map(Match::seed));
			}
		}
		if let Some(game) = &mut self.game {
			game.set_room(&self.room.members, self.room.owner);
		}
		self.update_info();
	}

	/// Apply a message that the bus delivered from the member `sender`, at the time `now` on the
	/// clock of this machine, in milliseconds.
	pub fn receive(&mut self, now: f64, sender: u32, message: &Message) {
		match message {
			Message::Profile { name } => {
				if self.room.members.contains(&sender) {
					self.names.insert(sender, clean_name(name));
				}
			}
			Message::Lobby { settings, playing } if sender == self.room.owner => {
				self.settings = Settings {
					bots: settings.bots.min(MAX_BOTS),
					skill: settings.skill,
				};
				match (playing, &self.game) {
					(Some(seed), None) => {
						let mut game = Match::new(self.room.you, *seed, self.settings);
						game.set_room(&self.room.members, self.room.owner);
						(self.game, self.results) = (Some(game), None);
					}
					(None, Some(_)) => self.end_match(),
					_ => {}
				}
				self.update_info();
			}
			Message::EndMatch if sender == self.room.owner => {
				self.end_match();
				self.update_info();
			}
			Message::Lobby { .. } | Message::EndMatch => {}
			_ => {
				if let Some(game) = &mut self.game {
					game.receive(now, sender, message);
				}
			}
		}
	}

	/// Advance a running match by one step, with the controls of the player.
	pub fn step(&mut self, now: f64, input: &Controls) {
		if let Some(game) = &mut self.game {
			game.step(now, input);
			self.messages.append(&mut game.drain_messages());
		}
	}

	/// Take the messages that this machine must send to the room, in their order.
	pub fn drain_messages(&mut self) -> Vec<Message> {
		std::mem::take(&mut self.messages)
	}

	/// Take the requests that this machine, as the host, must send to the bus, in their order.
	pub fn drain_controls(&mut self) -> Vec<Control> {
		std::mem::take(&mut self.controls)
	}

	/// Deliver the messages of this session to itself, as a room with one member does.
	///
	/// A solo game has no bus, and it calls this after each change and each step.
	pub fn deliver_to_self(&mut self, now: f64) {
		self.controls.clear();
		while !self.messages.is_empty() {
			for message in self.drain_messages() {
				self.receive(now, self.room.you, &message);
			}
		}
	}

	/// Choose the bots of the next match. Only the host can, and only in the lobby.
	pub fn set_settings(&mut self, settings: Settings) {
		if self.is_host() && self.game.is_none() {
			self.settings = Settings {
				bots: settings.bots.min(MAX_BOTS),
				skill: settings.skill,
			};
			self.announce_lobby(None);
			self.update_info();
		}
	}

	/// Start a match with this seed. Only the host can, and only in the lobby.
	///
	/// The match starts on every machine, this one included, when the bus delivers the message.
	pub fn start_match(&mut self, seed: u32) {
		if self.is_host() && self.game.is_none() {
			self.announce_lobby(Some(seed));
		}
	}

	/// End the match. Only the host can. Every machine then shows the results.
	pub fn request_end(&mut self) {
		if self.is_host() && self.game.is_some() {
			self.messages.push(Message::EndMatch);
		}
	}

	/// Rename the room. Only the host can.
	pub fn rename_room(&mut self, title: &str) {
		if self.is_host() {
			self.controls.push(Control::Title(title.to_owned()));
		}
	}

	/// Remove the member with this number from the room. Only the host can, and not itself.
	pub fn remove_member(&mut self, number: u32) {
		if self.is_host() && number != self.room.you && self.room.members.contains(&number) {
			self.controls.push(Control::Kick(number));
		}
	}

	/// Leave the results, and return to the lobby.
	pub fn close_results(&mut self) {
		self.results = None;
	}

	/// Return what the session shows now.
	#[must_use]
	pub fn phase(&self) -> Phase {
		match (&self.game, &self.results) {
			(Some(_), _) => Phase::Match,
			(None, Some(_)) => Phase::Results,
			(None, None) => Phase::Lobby,
		}
	}

	/// Return the status of the room, as the bus gave it last.
	#[must_use]
	pub fn room(&self) -> &RoomStatus {
		&self.room
	}

	/// Return whether this machine hosts the room.
	#[must_use]
	pub fn is_host(&self) -> bool {
		self.room.owner == self.room.you
	}

	/// Return the choices of the host for the next match, or of the match that runs.
	#[must_use]
	pub fn settings(&self) -> Settings {
		self.settings
	}

	/// Return the members of the room with their names, in the order in which they joined.
	#[must_use]
	pub fn players(&self) -> Vec<(u32, String)> {
		self.room
			.members
			.iter()
			.map(|&number| (number, self.name_of(PlaneId::Player(number))))
			.collect()
	}

	/// Return the name of the pilot of a plane. A player that did not name itself yet has a
	/// name from its number.
	#[must_use]
	pub fn name_of(&self, plane: PlaneId) -> String {
		match plane {
			PlaneId::Player(number) => match self.names.get(&number).filter(|name| !name.is_empty()) {
				Some(name) => name.clone(),
				None => format!("Player {number}"),
			},
			PlaneId::Bot(index) => format!("Bot {}", index + 1),
		}
	}

	/// Return the match that runs.
	#[must_use]
	pub fn game(&self) -> Option<&Match> {
		self.game.as_ref()
	}

	/// Return the match that runs, for a change. This is the hook of tests and playtests.
	pub fn game_mut(&mut self) -> Option<&mut Match> {
		self.game.as_mut()
	}

	/// Return the results of the last match, with the most kills first, until the player closes them.
	#[must_use]
	pub fn results(&self) -> Option<&[Score]> {
		self.results.as_deref()
	}

	/// Return the scores of the match that runs, with the most kills first.
	#[must_use]
	pub fn scores(&self) -> Vec<Score> {
		let mut scores: Vec<Score> = self
			.game
			.iter()
			.flat_map(Match::planes)
			.map(|(plane, state)| Score {
				plane,
				name: self.name_of(plane),
				kills: state.kills,
				deaths: state.deaths,
			})
			.collect();
		scores.sort_by(|a, b| {
			b.kills
				.cmp(&a.kills)
				.then(a.deaths.cmp(&b.deaths))
				.then(a.plane.cmp(&b.plane))
		});
		scores
	}

	fn announce_lobby(&mut self, playing: Option<u32>) {
		self.messages.push(Message::Lobby {
			settings: self.settings,
			playing,
		});
	}

	fn end_match(&mut self) {
		if self.game.is_some() {
			self.results = Some(self.scores());
			self.game = None;
		}
	}

	/// As the host, tell the bus what the room does, for the room list.
	fn update_info(&mut self) {
		if !self.is_host() {
			return;
		}
		let Settings { bots, skill } = self.settings;
		let doing = if self.game.is_some() {
			"In a match"
		} else {
			"In the lobby"
		};
		let info = match bots {
			0 => format!("{doing}, no bots"),
			1 => format!("{doing}, 1 bot ({})", skill.name()),
			_ => format!("{doing}, {bots} bots ({})", skill.name()),
		};
		if info != self.info {
			self.info.clone_from(&info);
			self.controls.push(Control::Info(info));
		}
	}
}

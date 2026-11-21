//! Several machines on one bus in memory, for the tests of a match.
#![allow(dead_code, reason = "each test file uses a part of these helpers")]

use glam::{Quat, Vec3};
use sim::{
	Controls, Match, Plane, Session,
	room::{Control, RoomStatus, clean_title},
	tuning,
	wire::{Message, PlaneId, Settings},
};

/// The controls of a pilot that flies straight toward `aim` at full throttle.
pub fn toward(aim: Vec3) -> Controls {
	Controls {
		aim,
		throttle: 1.0,
		boost: false,
		fire: false,
		launch: false,
	}
}

/// The machines of one room. The bus gives every message to every machine, in one order.
pub struct Bus {
	pub numbers: Vec<u32>,
	pub machines: Vec<Match>,
	/// The clock of all machines, in milliseconds.
	pub now: f64,
	/// Every message that the bus carried and that is no state, with its sender.
	pub log: Vec<(u32, Message)>,
}

impl Bus {
	/// A room with one machine for each number. The first number hosts the room.
	pub fn new(numbers: &[u32], settings: Settings) -> Self {
		let machines = numbers
			.iter()
			.map(|&me| {
				let mut game = Match::new(me, 7, settings);
				game.set_room(numbers, numbers[0]);
				game
			})
			.collect();
		Self {
			numbers: numbers.to_vec(),
			machines,
			now: 1000.0,
			log: Vec::new(),
		}
	}

	/// The machine of the player with this number.
	pub fn machine(&mut self, number: u32) -> &mut Match {
		let index = self
			.numbers
			.iter()
			.position(|&n| n == number)
			.expect("no machine has this number");
		&mut self.machines[index]
	}

	/// Put the plane of player `number` at `position` with `orientation`, whole and without protection.
	pub fn place(&mut self, number: u32, position: Vec3, orientation: Quat) {
		let plane = self
			.machine(number)
			.plane_mut(PlaneId::Player(number))
			.expect("each machine flies its player");
		*plane = Plane::new(position, orientation);
	}

	/// Run one step on every machine, and deliver the messages of the step to every machine.
	pub fn step(&mut self, mut controls: impl FnMut(u32, &Match) -> Controls) {
		let mut frames = Vec::new();
		for (&number, machine) in self.numbers.iter().zip(&mut self.machines) {
			let input = controls(number, machine);
			machine.step(self.now, &input);
			frames.extend(
				machine
					.drain_messages()
					.into_iter()
					.map(|message| (number, message.encode())),
			);
		}
		for (sender, bytes) in frames {
			let message = Message::decode(&bytes).expect("a machine sent bytes that are no message");
			for machine in &mut self.machines {
				machine.receive(self.now, sender, &message);
			}
			if !message.is_state() {
				self.log.push((sender, message));
			}
		}
		self.now += f64::from(tuning::STEP) * 1000.0;
	}

	/// Run the room for `seconds`.
	pub fn run(&mut self, seconds: f32, mut controls: impl FnMut(u32, &Match) -> Controls) {
		for _ in 0..(seconds / tuning::STEP).round() as u32 {
			self.step(&mut controls);
		}
	}
}

/// A room of the bus with the sessions of its members, as the server runs it.
///
/// It gives each member a number, keeps the members in the order in which they joined, names the
/// lowest number as the owner, and delivers every message to every member in one order.
pub struct Room {
	/// The sessions of the members, in the order in which they joined.
	pub sessions: Vec<Session>,
	next_number: u32,
	title: String,
	/// The clock of all machines, in milliseconds.
	pub now: f64,
	/// Every message that the room carried and that is no state, with its sender.
	pub log: Vec<(u32, Message)>,
	/// The info line that the owner set last.
	pub info: String,
}

impl Room {
	pub fn new() -> Self {
		Self {
			sessions: Vec::new(),
			next_number: 7,
			title: "Room 1".into(),
			now: 1000.0,
			log: Vec::new(),
			info: String::new(),
		}
	}

	fn members(&self) -> Vec<u32> {
		self.sessions.iter().map(|session| session.room().you).collect()
	}

	fn status(&self, members: &[u32], you: u32) -> RoomStatus {
		let owner = members.iter().copied().min().unwrap_or(you);
		RoomStatus {
			id: "1".into(),
			title: self.title.clone(),
			owner,
			members: members.to_vec(),
			you,
		}
	}

	fn send_status(&mut self) {
		let members = self.members();
		for index in 0..self.sessions.len() {
			let status = self.status(&members, members[index]);
			self.sessions[index].room_changed(status);
		}
	}

	/// Let a player with this name join, and return the number that the bus gives it.
	pub fn join(&mut self, name: &str) -> u32 {
		let number = self.next_number;
		self.next_number += 1;
		let mut members = self.members();
		members.push(number);
		let status = self.status(&members, number);
		self.sessions.push(Session::new(name, status));
		self.send_status();
		self.deliver();
		number
	}

	/// Let the member with this number leave.
	pub fn leave(&mut self, number: u32) {
		self.sessions.retain(|session| session.room().you != number);
		self.send_status();
		self.deliver();
	}

	/// The session of the member with this number.
	pub fn session(&mut self, number: u32) -> &mut Session {
		self.sessions
			.iter_mut()
			.find(|session| session.room().you == number)
			.expect("no member has this number")
	}

	/// Deliver what the sessions want to send, until no session sends more.
	pub fn deliver(&mut self) {
		loop {
			let mut frames = Vec::new();
			let mut requests = Vec::new();
			for session in &mut self.sessions {
				let (number, owner) = (session.room().you, session.room().owner);
				frames.extend(
					session
						.drain_messages()
						.into_iter()
						.map(|message| (number, message.encode())),
				);
				// The bus acts on the requests of the owner, and ignores those of other members.
				requests.extend(session.drain_controls().into_iter().filter(|_| number == owner));
			}
			if frames.is_empty() && requests.is_empty() {
				return;
			}
			for (sender, bytes) in frames {
				let message = Message::decode(&bytes).expect("a session sent bytes that are no message");
				for session in &mut self.sessions {
					session.receive(self.now, sender, &message);
				}
				if !message.is_state() {
					self.log.push((sender, message));
				}
			}
			for request in requests {
				match request {
					Control::Info(info) => self.info = info,
					Control::Title(title) => {
						self.title = clean_title(&title, "1");
						self.send_status();
					}
					Control::Kick(number) => {
						self.sessions.retain(|session| session.room().you != number);
						self.send_status();
					}
				}
			}
		}
	}

	/// Run one step on every machine, and deliver the messages of the step.
	pub fn step(&mut self, mut controls: impl FnMut(u32, &Session) -> Controls) {
		for session in &mut self.sessions {
			let input = controls(session.room().you, session);
			session.step(self.now, &input);
		}
		self.deliver();
		self.now += f64::from(tuning::STEP) * 1000.0;
	}

	/// Run the room for `seconds`.
	pub fn run(&mut self, seconds: f32, mut controls: impl FnMut(u32, &Session) -> Controls) {
		for _ in 0..(seconds / tuning::STEP).round() as u32 {
			self.step(&mut controls);
		}
	}
}

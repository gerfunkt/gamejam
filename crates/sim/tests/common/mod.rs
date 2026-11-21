//! Several machines on one bus in memory, for the tests of a match.
#![allow(dead_code, reason = "each test file uses a part of these helpers")]

use glam::{Quat, Vec3};
use sim::{
	Controls, Match, Plane, tuning,
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

//! The messages between the machines of a match.
//!
//! The bus carries each message as one binary frame in the wire format of Postcard 1.0
//! (<https://postcard.jamesmunns.com/wire-format>). A machine sends its states often, and may
//! lose one. It sends every other message once, and each machine applies those in the order of
//! the bus.

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::{bot::Skill, plane::Part};

/// The most characters of the name of a player.
pub const MAX_NAME_CHARS: usize = 16;
/// The most bots in one match.
pub const MAX_BOTS: u8 = 8;

/// The identity of a plane in a match.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PlaneId {
	/// The plane of the player to whom the bus gave this number.
	Player(u32),
	/// The bot with this index. The host of the room flies the bots.
	Bot(u8),
}

/// What every machine knows about a plane at one moment.
///
/// The machine that flies a plane sends this state, and every other machine draws the plane,
/// aims at it, and decides its own hits on it from this state.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaneState {
	/// The position of the centre of the fuselage in the world, in metres.
	pub position: Vec3,
	/// The rotation from the frame of the plane to the world.
	pub orientation: Quat,
	/// The speed along the nose, in m/s.
	pub speed: f32,
	/// The health of each part, rounded up, in the order of [`Part::ALL`]. A broken part has 0.
	pub health: [u8; Part::ALL.len()],
	/// Whether the plane flies. A destroyed plane waits for its respawn.
	pub alive: bool,
	/// Whether the plane is protected from damage after its spawn.
	pub protected: bool,
	/// Whether the plane fires its gun.
	pub firing: bool,
	/// Whether the plane spends boost.
	pub boosting: bool,
	/// The plane at which a missile lock of this plane points, while the lock closes or holds.
	pub lock: Option<PlaneId>,
	/// The planes that this plane destroyed in the match.
	pub kills: u16,
	/// How often this plane was destroyed in the match.
	pub deaths: u16,
}

impl PlaneState {
	/// Return the direction of the nose in the world.
	#[must_use]
	pub fn forward(&self) -> Vec3 {
		self.orientation * Vec3::NEG_Z
	}

	/// Return the velocity in the world, in m/s.
	#[must_use]
	pub fn velocity(&self) -> Vec3 {
		self.forward() * self.speed
	}

	/// Return whether `part` broke off.
	#[must_use]
	pub fn is_broken(&self, part: Part) -> bool {
		self.health[part as usize] == 0
	}

	/// Return whether a shot can damage the plane now.
	#[must_use]
	pub fn can_be_hit(&self) -> bool {
		self.alive && !self.protected
	}
}

/// The choices of the host for the next match.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
	/// The number of bots, from 0 to [`MAX_BOTS`].
	pub bots: u8,
	/// The skill of every bot.
	pub skill: Skill,
}

impl Default for Settings {
	fn default() -> Self {
		Self {
			bots: 4,
			skill: Skill::Pilot,
		}
	}
}

/// One message between the machines of a room.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Message {
	/// The state of a plane that the sender flies.
	State {
		/// The plane.
		plane: PlaneId,
		/// The clock of the sender when it sent the state, in milliseconds. It keeps its pace
		/// when the sender stalls, so a receiver can place the state in time.
		clock: u32,
		/// The state.
		state: PlaneState,
	},
	/// A shot of a plane of the sender that hit. The machine that flies the target applies it.
	Hit {
		/// The plane that was hit.
		target: PlaneId,
		/// The part that was hit.
		part: Part,
		/// The health that the hit takes.
		damage: f32,
		/// The plane that shot.
		by: PlaneId,
	},
	/// The end of a plane that the sender flies.
	Kill {
		/// The plane that was destroyed.
		victim: PlaneId,
		/// The plane that gets the kill. A crash without an attacker has none.
		killer: Option<PlaneId>,
	},
	/// A missile that a plane of the sender launched. Other machines draw it from here on.
	Launch {
		/// The plane that launched.
		shooter: PlaneId,
		/// The number of the missile among those of the shooter.
		missile: u8,
		/// The plane that the missile follows.
		target: PlaneId,
		/// Where the missile starts, in metres.
		position: Vec3,
		/// The direction in which it starts.
		direction: Vec3,
	},
	/// The end of a missile of the sender: a burst near its target, or empty fuel.
	Burst {
		/// The plane that launched the missile.
		shooter: PlaneId,
		/// The number of the missile among those of the shooter.
		missile: u8,
		/// Where the missile ended, in metres.
		position: Vec3,
	},
	/// A flare that a plane of the sender released. A missile that follows the plane can follow it instead.
	Flare {
		/// The plane that released the flare.
		plane: PlaneId,
		/// Where the flare starts, in metres.
		position: Vec3,
		/// The velocity with which it starts, in m/s.
		velocity: Vec3,
	},
	/// A claim of a plane of the sender on the power-up at a place. Every machine gives the power-up
	/// to the plane of the first claim that the bus delivers. A later claim for a power-up that is
	/// gone does nothing.
	Claim {
		/// The place of the power-up, as its index in [`crate::Match::power_ups`].
		place: u8,
		/// The plane that claims the power-up.
		plane: PlaneId,
	},
	/// The host ends the match, and every player sees the results.
	EndMatch,
	/// The lobby of the room, as its host holds it.
	Lobby {
		/// The choices for the next match, or of the match that runs.
		settings: Settings,
		/// The seed of the match that runs. A room that waits in its lobby has none.
		playing: Option<u32>,
	},
	/// The name of the sender.
	Profile {
		/// The name. A receiver cuts it to [`MAX_NAME_CHARS`].
		name: String,
	},
}

impl Message {
	/// Return the bytes of this message for the bus.
	#[must_use]
	pub fn encode(&self) -> Vec<u8> {
		postcard::to_allocvec(self).expect("a message holds no type that Postcard refuses")
	}

	/// Read a message from the bytes that the bus delivered.
	///
	/// Bytes that are no message, or that continue after a message, give `None`, because any
	/// member of a room can send anything.
	#[must_use]
	pub fn decode(bytes: &[u8]) -> Option<Self> {
		let (message, rest) = postcard::take_from_bytes(bytes).ok()?;
		rest.is_empty().then_some(message)
	}

	/// Return whether this message is a state, which may be lost, and not an event.
	#[must_use]
	pub fn is_state(&self) -> bool {
		matches!(self, Message::State { .. })
	}
}

/// Return the name that a player gets from the text `raw`: trimmed, and cut to [`MAX_NAME_CHARS`].
#[must_use]
pub fn clean_name(raw: &str) -> String {
	raw.trim().chars().take(MAX_NAME_CHARS).collect()
}

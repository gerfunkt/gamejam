use serde::{Deserialize, Serialize};

/// How well a bot flies and shoots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Skill {
	/// A beginner: slow turns, a wide aim, and no missiles.
	Rookie,
	/// An average pilot.
	Pilot,
	/// A good pilot.
	Veteran,
	/// The best pilot.
	Ace,
}

impl Skill {
	/// Every skill, from the weakest to the strongest.
	pub const ALL: [Skill; 4] = [Skill::Rookie, Skill::Pilot, Skill::Veteran, Skill::Ace];
}

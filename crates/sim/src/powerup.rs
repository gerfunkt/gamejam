//! Power-ups: icons that float at fixed places in the sky, and that a plane takes by flying through them.
//!
//! The places and the kinds are the same on every machine. The machine that flies a plane claims
//! a power-up for it, and every machine gives the power-up to the first claim that the bus
//! delivers, so all machines agree on the winner. The power-up returns after a time. A player
//! that joins a running match does not know which power-ups are gone.

use glam::Vec3;

use crate::tuning;

/// What a power-up gives to the plane that takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PowerUpKind {
	/// A faster plane for some seconds.
	Speed,
	/// A full rack of missiles and a full rack of flares.
	Weapons,
	/// Every part of the plane whole again.
	Repair,
}

/// The kind of each place, in the order of [`tuning::POWERUP_PLACES`].
const KINDS: [PowerUpKind; tuning::POWERUP_PLACES.len()] = [
	PowerUpKind::Speed,
	PowerUpKind::Weapons,
	PowerUpKind::Repair,
	PowerUpKind::Speed,
	PowerUpKind::Weapons,
	PowerUpKind::Repair,
	PowerUpKind::Speed,
	PowerUpKind::Weapons,
];

/// A power-up at its place in the sky.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PowerUp {
	/// What the power-up gives. The place fixes the kind.
	pub kind: PowerUpKind,
	/// The place in the world, in metres.
	pub position: Vec3,
	/// Whether the power-up is there now. A power-up that a plane took is gone until it returns.
	pub present: bool,
	/// The seconds until a power-up that is gone returns.
	back: f32,
	/// Whether this machine claimed the power-up, and waits for the bus to deliver the claim.
	asked: bool,
}

impl PowerUp {
	/// Return the power-ups of a new match: one at each place, all there.
	pub(crate) fn all() -> Vec<Self> {
		tuning::POWERUP_PLACES
			.iter()
			.zip(KINDS)
			.map(|(&place, kind)| Self {
				kind,
				position: Vec3::from(place),
				present: true,
				back: 0.0,
				asked: false,
			})
			.collect()
	}

	/// Return whether a plane at `position` takes the power-up, and this machine did not claim it yet.
	pub(crate) fn is_reached_by(&self, position: Vec3) -> bool {
		self.present && !self.asked && self.position.distance(position) <= tuning::POWERUP_RADIUS
	}

	/// Remember that this machine sent a claim for the power-up.
	pub(crate) fn ask(&mut self) {
		self.asked = true;
	}

	/// Take the power-up away until it returns.
	pub(crate) fn take(&mut self) {
		(self.present, self.back) = (false, tuning::POWERUP_RETURN_SECONDS);
	}

	/// Advance the return of a power-up that is gone by `dt` seconds.
	pub(crate) fn step(&mut self, dt: f32) {
		if self.present {
			return;
		}
		self.back -= dt;
		if self.back <= 0.0 {
			(self.present, self.asked) = (true, false);
		}
	}
}

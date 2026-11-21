use crate::tuning::{MAX_STEPS_PER_FRAME, STEP};

/// A counter that turns the time of each frame into whole steps of the rules.
///
/// The rules always advance by [`STEP`], so the game runs at one speed at every frame rate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FixedSteps {
	rest: f32,
}

impl FixedSteps {
	/// Add the time of one frame, in seconds, and return the number of steps to run now.
	///
	/// A frame runs at most [`MAX_STEPS_PER_FRAME`] steps. The time beyond that is dropped, so
	/// that a page that was hidden for a while does not run many steps at once.
	pub fn advance(&mut self, frame_seconds: f32) -> u32 {
		self.rest += frame_seconds.max(0.0);
		let steps = (self.rest / STEP) as u32;
		if steps > MAX_STEPS_PER_FRAME {
			self.rest = 0.0;
			return MAX_STEPS_PER_FRAME;
		}
		self.rest -= steps as f32 * STEP;
		steps
	}

	/// Return how far the current frame is between the last step and the next one, from 0 to 1.
	///
	/// A renderer draws between the last two states at this fraction, so that motion stays
	/// smooth when the frame rate and the step rate differ.
	#[must_use]
	pub fn fraction(&self) -> f32 {
		self.rest / STEP
	}
}

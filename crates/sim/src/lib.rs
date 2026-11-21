//! The rules of the game: flight, damage, guns, missiles, bots, and the messages between machines.
//!
//! Every machine runs these rules. The crate knows no engine, no screen, and no network, so its
//! tests play whole fights without a browser.
//!
//! The units are metres, seconds, and radians. The Y axis points up. A plane flies toward its
//! local -Z axis, with +X to its right and +Y above it, as in Bevy.

mod clock;
mod plane;
mod steer;
pub mod tuning;

pub use clock::FixedSteps;
pub use plane::{Controls, Part, Plane};
pub use steer::{Steering, aim_direction, level_frame, steer_toward};

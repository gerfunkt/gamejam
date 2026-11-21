//! The rules of the game, with no engine and no screen. Every machine
//! runs this code: the client for its own plane, and the tests for
//! whole fights.

pub mod bots;
pub mod combat;
pub mod flight;
pub mod lobby;
pub mod missile;
pub mod room;
pub mod timeline;
pub mod tuning;
pub mod wire;
pub mod world;

pub use glam::{Quat, Vec3};

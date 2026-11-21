//! The record of where an asset comes from.

use serde::Deserialize;

/// Where a model comes from, as the game-assets rules ask. Every field is written, even as "not recorded".
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Record {
	/// Who made the model.
	pub author: String,
	/// The service or the tool that made it.
	pub service: String,
	/// The date on which it was made, as `YYYY-MM-DD`.
	pub date: String,
	/// The prompt that the service got.
	pub prompt: String,
	/// The licence terms that applied to the model on that date.
	pub licence: String,
}

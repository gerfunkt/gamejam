//! The manifest of the plane models: `assets/planes/planes.toml`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::propeller::Stretch;
use crate::{mesh::orient::SignedAxis, record::Record};

/// One plane model.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Plane {
	/// The stable name of the model. It names the output file.
	pub key: String,
	/// The name of the plane that the player sees.
	pub name: String,
	/// The FBX file, relative to the manifest.
	pub model: PathBuf,
	/// The PNG texture of the model, relative to the manifest.
	pub texture: PathBuf,
	/// The axis of the model that points to the nose.
	pub nose: SignedAxis,
	/// The axis of the model that points up.
	pub up: SignedAxis,
	/// The angle in degrees by which to lift the nose, to level a model that stands on its wheels. Default 0.
	#[serde(default)]
	pub pitch: f32,
	/// The number of kills from which a pilot flies this model.
	pub kills: u32,
	/// Where the blades of the propeller are, if the model has one. The build takes them out, and the client draws a blur.
	pub propeller: Option<Stretch>,
	/// The origin of the model.
	pub record: Record,
}

/// The planes of the manifest, in the order of the file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
	plane: Vec<Plane>,
}

/// Read the manifest text, and check that its models can be told apart.
///
/// # Errors
///
/// Returns an error if the text is not a valid manifest, if two entries share a key or a number of
/// kills, if a key is not a file name in lowercase letters, digits, and hyphens, if a propeller ends
/// before it starts, or if no entry starts at 0 kills, because a new pilot needs a model.
pub fn parse(text: &str) -> Result<Vec<Plane>> {
	let Manifest { plane: planes } = toml::from_str(text).context("planes.toml is not valid")?;
	for (index, plane) in planes.iter().enumerate() {
		let key = &plane.key;
		ensure!(
			!key.is_empty()
				&& key
					.bytes()
					.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'),
			"the key `{key}` must use lowercase letters, digits, and hyphens, because it names a file"
		);
		for other in &planes[..index] {
			ensure!(other.key != *key, "the key `{key}` appears twice");
			ensure!(
				other.kills != plane.kills,
				"`{}` and `{key}` both start at {} kills; give them different numbers",
				other.key,
				plane.kills
			);
		}
	}
	for plane in &planes {
		if let Some(stretch) = plane.propeller {
			ensure!(
				stretch.from < stretch.to,
				"`{}`: the propeller must start before it ends, and `from` is {} and `to` is {}",
				plane.key,
				stretch.from,
				stretch.to
			);
		}
	}
	ensure!(
		planes.iter().any(|plane| plane.kills == 0),
		"no model starts at 0 kills; a new pilot needs one"
	);
	Ok(planes)
}

/// Read and check the manifest file.
///
/// # Errors
///
/// Returns an error if the file cannot be read, or if [`parse`] refuses its text.
pub fn read(path: &Path) -> Result<Vec<Plane>> {
	let text = std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
	parse(&text)
}

#[cfg(test)]
mod tests {
	use super::*;

	const ENTRY: &str = r#"
[[plane]]
key = "{key}"
name = "Test Plane"
model = "m.fbx"
texture = "t.png"
nose = "-Z"
up = "+Y"
kills = {kills}
[plane.record]
author = "a"
service = "s"
date = "2026-01-01"
prompt = "not recorded"
licence = "not recorded"
"#;

	fn entry(key: &str, kills: u32) -> String {
		ENTRY.replace("{key}", key).replace("{kills}", &kills.to_string())
	}

	#[test]
	fn the_manifest_of_the_repository_is_valid() {
		let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/planes/planes.toml");
		let planes = read(&path).unwrap();
		assert_eq!(planes[0].key, "wwi-fighter", "the first model of the game");
		assert_eq!(planes[0].name, "Sopwith Camel", "the name on the screen");
		assert_eq!(planes[0].kills, 0, "a new pilot flies it");
		assert_eq!(planes[1].kills, 3, "three kills earn the second model");
	}

	#[test]
	fn a_pitch_is_read_and_defaults_to_zero() {
		let with_pitch = entry("a", 0).replace("kills = 0", "pitch = -12.5\nkills = 0");
		assert_eq!(parse(&with_pitch).unwrap()[0].pitch, -12.5, "the pitch of the entry");
		assert_eq!(parse(&entry("a", 0)).unwrap()[0].pitch, 0.0, "no pitch means level");
	}

	#[test]
	fn a_propeller_is_read_and_must_start_before_it_ends() {
		let with = |propeller: &str| entry("a", 0).replace("kills = 0", &format!("kills = 0\npropeller = {propeller}"));
		assert_eq!(
			parse(&with("{ from = 3.5, to = 3.9 }")).unwrap()[0].propeller,
			Some(Stretch { from: 3.5, to: 3.9 }),
			"the stretch of the entry"
		);
		assert_eq!(
			parse(&entry("a", 0)).unwrap()[0].propeller,
			None,
			"a jet has no propeller"
		);
		let message = parse(&with("{ from = 3.9, to = 3.5 }")).unwrap_err().to_string();
		assert!(message.contains("`a`"), "the message names the model: {message}");
	}

	#[test]
	fn two_models_are_read_in_the_order_of_the_file() {
		let planes = parse(&(entry("first", 0) + &entry("second", 5))).unwrap();
		let keys: Vec<&str> = planes.iter().map(|plane| plane.key.as_str()).collect();
		assert_eq!(keys, ["first", "second"], "the order of the file");
		assert_eq!(planes[1].kills, 5, "the kills of the second model");
	}

	#[test]
	fn a_duplicate_key_is_refused() {
		assert!(parse(&(entry("a", 0) + &entry("a", 1))).is_err(), "keys are unique");
	}

	#[test]
	fn two_models_with_the_same_kills_are_refused() {
		assert!(
			parse(&(entry("a", 0) + &entry("b", 0))).is_err(),
			"one model for each number of kills"
		);
	}

	#[test]
	fn a_manifest_without_a_model_for_new_pilots_is_refused() {
		assert!(parse(&entry("a", 3)).is_err(), "someone must fly with 0 kills");
	}

	#[test]
	fn a_key_that_is_not_a_file_name_is_refused() {
		assert!(
			parse(&entry("../etc", 0)).is_err(),
			"a key must not climb out of the directory"
		);
		assert!(parse(&entry("Big Plane", 0)).is_err(), "no spaces and no capitals");
	}

	#[test]
	fn a_record_field_that_is_missing_is_refused() {
		let text = entry("a", 0).replace("licence = \"not recorded\"\n", "");
		assert!(
			parse(&text).is_err(),
			"the licence must be written, even as `not recorded`"
		);
	}
}

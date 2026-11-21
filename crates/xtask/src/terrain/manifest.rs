//! The manifest of the terrain: `assets/terrain/terrain.toml`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{mesh::orient::SignedAxis, record::Record};

/// The terrain map, with the files that the bake reads and the numbers of the bake.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Terrain {
	/// The stable name of the map.
	pub key: String,
	/// The name of the map for people.
	pub name: String,
	/// The file name of the archive that the raw files come from. The archive is not in git.
	pub archive: String,
	/// The size of the archive, in bytes.
	pub archive_bytes: u64,
	/// The SHA-256 of the archive, as 64 hexadecimal digits in lowercase.
	pub archive_sha256: String,
	/// The FBX file, relative to the manifest.
	pub model: PathBuf,
	/// The base colour PNG texture of the model, relative to the manifest.
	pub texture: PathBuf,
	/// The axis of the model that points to -Z of the world.
	pub north: SignedAxis,
	/// The axis of the model that points up.
	pub up: SignedAxis,
	/// The number of points on a side of the height grid.
	pub points: usize,
	/// The number of pixels on a side of the colour picture.
	pub picture: usize,
	/// The origin of the model.
	pub record: Record,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
	terrain: Terrain,
}

/// Read the manifest text, and check its numbers.
///
/// # Errors
///
/// Returns an error if the text is not a valid manifest, if the grid has fewer than 2 points or more
/// than 65535 on a side, if the picture has no pixel, or if the SHA-256 is not 64 lowercase hexadecimal digits.
pub fn parse(text: &str) -> Result<Terrain> {
	let Manifest { terrain } = toml::from_str(text).context("terrain.toml is not valid")?;
	ensure!(
		(2..=usize::from(u16::MAX)).contains(&terrain.points),
		"`points` is {}; the height grid needs from 2 to 65535 points on a side",
		terrain.points
	);
	ensure!(terrain.picture > 0, "`picture` must be at least 1 pixel");
	ensure!(
		terrain.archive_sha256.len() == 64
			&& terrain
				.archive_sha256
				.bytes()
				.all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
		"`archive_sha256` must be 64 hexadecimal digits in lowercase"
	);
	Ok(terrain)
}

/// Read and check the manifest file.
///
/// # Errors
///
/// Returns an error if the file cannot be read, or if [`parse`] refuses its text.
pub fn read(path: &Path) -> Result<Terrain> {
	let text = std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
	parse(&text)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn entry() -> String {
		format!(
			"[terrain]\nkey = \"map\"\nname = \"A map\"\narchive = \"map.zip\"\narchive_bytes = 7\narchive_sha256 = \"{}\"\n\
			 model = \"raw/model.fbx\"\ntexture = \"raw/model.png\"\nnorth = \"-Z\"\nup = \"+Y\"\npoints = 257\npicture = 1024\n\
			 [terrain.record]\nauthor = \"a\"\nservice = \"s\"\ndate = \"d\"\nprompt = \"p\"\nlicence = \"l\"\n",
			"0123456789abcdef".repeat(4)
		)
	}

	#[test]
	fn a_complete_manifest_is_read() {
		let terrain = parse(&entry()).unwrap();
		assert_eq!((terrain.points, terrain.picture), (257, 1024), "the sizes");
		assert_eq!(terrain.model, PathBuf::from("raw/model.fbx"), "the model path");
	}

	#[test]
	fn the_manifest_of_the_repository_is_valid_and_names_files_of_the_raw_directory() {
		let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/terrain/terrain.toml");
		let terrain = read(&path).unwrap();
		assert!(
			terrain.model.starts_with("raw") && terrain.texture.starts_with("raw"),
			"the raw files"
		);
	}

	#[test]
	fn a_grid_without_a_cell_is_refused() {
		let message = parse(&entry().replace("points = 257", "points = 1"))
			.unwrap_err()
			.to_string();
		assert!(message.contains("points"), "the message names the field: {message}");
	}

	#[test]
	fn a_checksum_that_is_not_lowercase_hexadecimal_is_refused() {
		let upper = entry().replace("abcdef", "ABCDEF");
		assert!(parse(&upper).is_err(), "uppercase digits");
		assert!(
			parse(&entry().replace("0123456789abcdef", "xyz")).is_err(),
			"a short text"
		);
	}

	#[test]
	fn an_unknown_field_is_refused() {
		assert!(
			parse(&entry().replace("points = 257", "points = 257\nextra = 1")).is_err(),
			"a typo must not pass"
		);
	}
}

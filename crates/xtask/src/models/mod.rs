//! The plane models: from the FBX files of the artists to the GLB files of the client.
//!
//! `cargo xtask models` reads `assets/planes/planes.toml`. For each model, it turns the model to
//! the frame of the rules, scales it to the plane of the rules, cuts it into the five parts that
//! damage breaks, and writes `web/assets/planes/<key>.glb`. It also writes `planes.json`, which
//! lists the files for the client.

use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde_json::json;
use sim::Part;

use self::{
	fit::{Orientation, fit, orient, rules_box},
	parts::Regions,
};
use crate::mesh::{fbx, geometry::Triangle, scene};

mod fit;
mod glb;
mod manifest;
mod parts;
mod preview;

/// The most triangles that a model may have, over all parts. A phone draws every plane of a match.
const MAX_TRIANGLES: usize = 60_000;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// A model after the fit and the cut.
struct Model {
	/// The triangles of each part, in the order of `Part::ALL`.
	parts: Vec<(Part, Vec<Triangle>)>,
	/// The factor between the units of the source file and metres.
	scale: f32,
}

/// Turn the FBX file into the five parts of the plane of the rules.
fn convert(fbx: &[u8], orientation: Orientation) -> Result<Model> {
	let mut triangles = scene::read(&fbx::parse(fbx)?)?;
	orient(&mut triangles, orientation)?;
	let scale = fit(&mut triangles, rules_box())?;
	Ok(Model {
		parts: Regions::for_model(&triangles).split(&triangles),
		scale,
	})
}

/// Check the triangles of the parts against the budgets of the game.
fn check_budget(key: &str, parts: &[(Part, Vec<Triangle>)]) -> Result<()> {
	for (part, triangles) in parts {
		ensure!(
			!triangles.is_empty(),
			"{key}: {part:?} has no triangles. The model does not reach the box of that part. \
			 Check `nose` and `up` of the model in planes.toml, or ask the artist for a model with that part"
		);
	}
	let total: usize = parts.iter().map(|(_, triangles)| triangles.len()).sum();
	ensure!(
		total <= MAX_TRIANGLES,
		"{key} has {total} triangles, and the budget is {MAX_TRIANGLES}. Reduce the model in the tool of the artist, \
		 for example with the polygon count of the service, or with a decimate step in Blender"
	);
	Ok(())
}

/// Convert every model of the manifest, and write the GLB files and the list for the client.
///
/// With `preview`, it also writes `assets/planes/<key>-preview.png` for each model.
pub fn run(root: &Path, preview: bool) -> Result<()> {
	let assets = root.join("assets/planes");
	let planes = manifest::read(&assets.join("planes.toml"))?;
	let output = root.join("web/assets/planes");
	fs::create_dir_all(&output).with_context(|| format!("cannot create {}", output.display()))?;
	for plane in &planes {
		let fbx =
			fs::read(assets.join(&plane.model)).with_context(|| format!("cannot read the model of {}", plane.key))?;
		let texture = fs::read(assets.join(&plane.texture))
			.with_context(|| format!("cannot read the texture of {}", plane.key))?;
		ensure!(
			texture.starts_with(PNG_SIGNATURE),
			"{}: the texture is not a PNG file",
			plane.key
		);
		let model = convert(
			&fbx,
			Orientation {
				nose: plane.nose,
				up: plane.up,
				pitch: plane.pitch,
			},
		)
		.with_context(|| format!("cannot convert the model of {}", plane.key))?;
		check_budget(&plane.key, &model.parts)?;
		let glb = glb::encode(&model.parts, &texture)?;
		let path = output.join(format!("{}.glb", plane.key));
		fs::write(&path, &glb).with_context(|| format!("cannot write {}", path.display()))?;
		if preview {
			let path = assets.join(format!("{}-preview.png", plane.key));
			fs::write(&path, preview::render(&model.parts)?)
				.with_context(|| format!("cannot write {}", path.display()))?;
		}
		let counts: Vec<String> = model
			.parts
			.iter()
			.map(|(part, triangles)| format!("{part:?} {}", triangles.len()))
			.collect();
		println!(
			"{}: {} triangles ({}), scale {:.5}, {:.1} MB",
			plane.key,
			model.parts.iter().map(|(_, triangles)| triangles.len()).sum::<usize>(),
			counts.join(", "),
			model.scale,
			glb.len() as f64 / 1e6
		);
	}
	fs::write(
		output.join("planes.json"),
		serde_json::to_string_pretty(&list(&planes))?,
	)?;
	Ok(())
}

/// Return the list of the models for the client, in the order of the kills.
fn list(planes: &[manifest::Plane]) -> serde_json::Value {
	let mut ordered: Vec<&manifest::Plane> = planes.iter().collect();
	ordered.sort_by_key(|plane| plane.kills);
	ordered
		.into_iter()
		.map(|plane| json!({"key": plane.key, "file": format!("{}.glb", plane.key), "kills": plane.kills}))
		.collect()
}

#[cfg(test)]
mod tests {
	use std::path::PathBuf;

	use glam::Vec3;

	use super::*;
	use crate::mesh::geometry::tests::vertex;

	fn repository() -> PathBuf {
		Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
	}

	#[test]
	fn the_real_model_gives_five_parts_that_lie_in_their_regions_and_in_the_plane_of_the_rules() {
		let assets = repository().join("assets/planes");
		let planes = manifest::read(&assets.join("planes.toml")).unwrap();
		let plane = &planes[0];
		let fbx = fs::read(assets.join(&plane.model)).unwrap();
		let model = convert(
			&fbx,
			Orientation {
				nose: plane.nose,
				up: plane.up,
				pitch: plane.pitch,
			},
		)
		.unwrap();
		let triangles = |part: Part| model.parts.iter().find(|(owner, _)| *owner == part).unwrap().1.len();
		let (left, right) = (triangles(Part::LeftWing), triangles(Part::RightWing));
		assert!(
			left.abs_diff(right) * 5 <= left.max(right),
			"the wings are about as big as each other, so the span is along X: {left} and {right} triangles"
		);
		check_budget(&plane.key, &model.parts).unwrap();
		let whole: Vec<Triangle> = model
			.parts
			.iter()
			.flat_map(|(_, triangles)| triangles.clone())
			.collect();
		let regions = Regions::for_model(&whole);
		let (low, high) = rules_box();
		let mut facing_outward = 0;
		let mut all = 0;
		for (part, triangles) in &model.parts {
			assert!(!triangles.is_empty(), "{part:?} has triangles");
			for triangle in triangles {
				let centroid = triangle.iter().map(|vertex| vertex.position).sum::<Vec3>() / 3.0;
				assert_eq!(
					regions.part_at(centroid),
					*part,
					"a triangle of {part:?} lies in the region of {part:?}"
				);
				for vertex in triangle {
					let slack = Vec3::splat(1e-3);
					assert!(
						vertex.position.cmpge(low.with_y(f32::MIN) - slack).all()
							&& vertex.position.cmple(high.with_y(f32::MAX) + slack).all(),
						"{part:?} stays inside the wing span and the length of the rules: {}",
						vertex.position
					);
				}
				let [a, b, c] = triangle.map(|vertex| vertex.position);
				let normal: Vec3 = triangle.iter().map(|vertex| vertex.normal).sum();
				all += 1;
				facing_outward += usize::from((b - a).cross(c - a).dot(normal) > 0.0);
			}
		}
		assert!(
			facing_outward * 100 >= all * 95,
			"the winding agrees with the normals for {facing_outward} of {all} triangles"
		);
	}

	#[test]
	fn a_part_without_triangles_fails_the_budget_with_a_hint() {
		let parts: Vec<_> = Part::ALL.map(|part| (part, vec![[vertex(0.0, 0.0, 0.0); 3]])).into();
		let mut empty = parts.clone();
		empty[3].1.clear();
		assert!(check_budget("x", &parts).is_ok(), "five parts with triangles pass");
		let message = check_budget("x", &empty).unwrap_err().to_string();
		assert!(
			message.contains("Tail") && message.contains("planes.toml"),
			"the message names the part and the fix: {message}"
		);
	}

	#[test]
	fn a_model_over_sixty_thousand_triangles_fails_the_budget_with_a_hint() {
		let many = vec![[vertex(0.0, 0.0, 0.0); 3]; MAX_TRIANGLES / 5 + 1];
		let parts: Vec<_> = Part::ALL.map(|part| (part, many.clone())).into();
		let message = check_budget("x", &parts).unwrap_err().to_string();
		assert!(
			message.contains("60000") && message.contains("Reduce"),
			"the message gives the budget and the fix: {message}"
		);
	}

	#[test]
	fn the_list_for_the_client_is_in_the_order_of_the_kills() {
		let entry = |key: &str, kills: u32| {
			format!(
				"[[plane]]\nkey = \"{key}\"\nmodel = \"m.fbx\"\ntexture = \"t.png\"\nnose = \"-Z\"\nup = \"+Y\"\nkills = {kills}\n\
				 [plane.record]\nauthor = \"a\"\nservice = \"s\"\ndate = \"d\"\nprompt = \"p\"\nlicence = \"l\"\n"
			)
		};
		let planes = manifest::parse(&(entry("ace", 10) + &entry("rookie", 0))).unwrap();
		let list = list(&planes);
		assert_eq!(
			list,
			json!([{"key": "rookie", "file": "rookie.glb", "kills": 0}, {"key": "ace", "file": "ace.glb", "kills": 10}]),
			"the models by the number of kills"
		);
	}
}

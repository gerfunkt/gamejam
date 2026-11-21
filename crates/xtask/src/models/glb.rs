//! Write the parts of a plane model as a glTF 2.0 binary file (GLB).
//!
//! The file follows the glTF 2.0 specification: a 12-byte header, a JSON chunk padded with spaces
//! to 4 bytes, and a binary chunk padded with zeros to 4 bytes. Every accessor and buffer view
//! starts at a multiple of 4 bytes.

use std::collections::HashMap;

use anyhow::{Result, ensure};
use glam::Vec3;
use serde_json::{Value, json};
use sim::Part;

use super::geometry::Triangle;

const MAGIC: u32 = 0x4654_6C67;
const JSON_CHUNK: u32 = 0x4E4F_534A;
const BINARY_CHUNK: u32 = 0x004E_4942;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;
const LINEAR: u32 = 9729;
const LINEAR_MIPMAP_LINEAR: u32 = 9987;
/// The roughness of the material. The planes are matt, and the light of the game is simple.
const ROUGHNESS: f64 = 0.9;

/// The binary buffer of the file, and the list of the views and the accessors into it.
#[derive(Default)]
struct Buffer {
	bytes: Vec<u8>,
	views: Vec<Value>,
	accessors: Vec<Value>,
}

impl Buffer {
	/// Append `data` as a new buffer view, and return the index of the view.
	fn view(&mut self, data: &[u8], target: Option<u32>) -> usize {
		debug_assert_eq!(self.bytes.len() % 4, 0, "the buffer is aligned to 4 bytes");
		let mut view = json!({"buffer": 0, "byteOffset": self.bytes.len(), "byteLength": data.len()});
		if let Some(target) = target {
			view["target"] = target.into();
		}
		self.views.push(view);
		self.bytes.extend_from_slice(data);
		self.bytes.resize(self.bytes.len().next_multiple_of(4), 0);
		self.views.len() - 1
	}

	/// Append an accessor over the data, and return its index.
	fn accessor(&mut self, data: &[u8], target: u32, component_type: u32, count: usize, kind: &str) -> usize {
		let view = self.view(data, Some(target));
		self.accessors.push(json!({
			"bufferView": view, "componentType": component_type, "count": count, "type": kind,
		}));
		self.accessors.len() - 1
	}
}

/// The vertices and the triangle indices of one part, with equal vertices merged.
struct Indexed {
	positions: Vec<Vec3>,
	normals: Vec<Vec3>,
	uvs: Vec<[f32; 2]>,
	indices: Vec<u32>,
}

fn weld(triangles: &[Triangle]) -> Result<Indexed> {
	let mut known: HashMap<[u32; 8], u32> = HashMap::new();
	let mut mesh = Indexed {
		positions: Vec::new(),
		normals: Vec::new(),
		uvs: Vec::new(),
		indices: Vec::new(),
	};
	for vertex in triangles.iter().flatten() {
		// glTF has the origin of the texture at the top left, and FBX has it at the bottom left.
		let uv = [vertex.uv.x, 1.0 - vertex.uv.y];
		let [px, py, pz] = vertex.position.to_array();
		let [nx, ny, nz] = vertex.normal.to_array();
		let key = [px, py, pz, nx, ny, nz, uv[0], uv[1]].map(f32::to_bits);
		let next = u32::try_from(mesh.positions.len())?;
		let index = *known.entry(key).or_insert_with(|| {
			mesh.positions.push(vertex.position);
			mesh.normals.push(vertex.normal);
			mesh.uvs.push(uv);
			next
		});
		mesh.indices.push(index);
	}
	Ok(mesh)
}

fn floats(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
	values.into_iter().flat_map(f32::to_le_bytes).collect()
}

/// Write `parts` and the PNG `texture` as a GLB file.
///
/// Each part becomes one mesh with the name of its `Part` variant, in one node at the origin. The
/// meshes share one material with the texture as its base colour, with metallic factor 0.
/// The texture coordinates are turned from the origin at the bottom left to the origin at the top left.
///
/// # Errors
///
/// Returns an error if a part has no triangles, because glTF cannot describe an empty mesh.
pub fn encode(parts: &[(Part, Vec<Triangle>)], texture: &[u8]) -> Result<Vec<u8>> {
	let mut buffer = Buffer::default();
	let (mut meshes, mut nodes) = (Vec::new(), Vec::new());
	for (index, (part, triangles)) in parts.iter().enumerate() {
		ensure!(!triangles.is_empty(), "{part:?} has no triangles");
		let mesh = weld(triangles)?;
		let count = mesh.positions.len();
		let low = mesh.positions.iter().fold(Vec3::MAX, |low, point| low.min(*point));
		let high = mesh.positions.iter().fold(Vec3::MIN, |high, point| high.max(*point));
		let position = buffer.accessor(
			&floats(mesh.positions.iter().flat_map(|point| point.to_array())),
			ARRAY_BUFFER,
			FLOAT,
			count,
			"VEC3",
		);
		buffer.accessors[position]["min"] = json!(low.to_array());
		buffer.accessors[position]["max"] = json!(high.to_array());
		let normal = buffer.accessor(
			&floats(mesh.normals.iter().flat_map(|vector| vector.to_array())),
			ARRAY_BUFFER,
			FLOAT,
			count,
			"VEC3",
		);
		let uv = buffer.accessor(
			&floats(mesh.uvs.iter().flatten().copied()),
			ARRAY_BUFFER,
			FLOAT,
			count,
			"VEC2",
		);
		let indices: Vec<u8> = mesh.indices.iter().flat_map(|index| index.to_le_bytes()).collect();
		let indices = buffer.accessor(
			&indices,
			ELEMENT_ARRAY_BUFFER,
			UNSIGNED_INT,
			mesh.indices.len(),
			"SCALAR",
		);
		meshes.push(json!({
			"name": format!("{part:?}"),
			"primitives": [{
				"attributes": {"POSITION": position, "NORMAL": normal, "TEXCOORD_0": uv},
				"indices": indices,
				"material": 0,
				"mode": 4,
			}],
		}));
		nodes.push(json!({"name": format!("{part:?}"), "mesh": index}));
	}
	let image = buffer.view(texture, None);
	let root = json!({
		"asset": {"version": "2.0", "generator": "cargo xtask models"},
		"scene": 0,
		"scenes": [{"nodes": (0..nodes.len()).collect::<Vec<_>>()}],
		"nodes": nodes,
		"meshes": meshes,
		"materials": [{
			"name": "plane",
			"pbrMetallicRoughness": {
				"baseColorTexture": {"index": 0},
				"metallicFactor": 0.0,
				"roughnessFactor": ROUGHNESS,
			},
		}],
		"textures": [{"sampler": 0, "source": 0}],
		"samplers": [{"magFilter": LINEAR, "minFilter": LINEAR_MIPMAP_LINEAR}],
		"images": [{"bufferView": image, "mimeType": "image/png"}],
		"buffers": [{"byteLength": buffer.bytes.len()}],
		"bufferViews": buffer.views,
		"accessors": buffer.accessors,
	});
	let mut json = serde_json::to_vec(&root)?;
	json.resize(json.len().next_multiple_of(4), b' ');
	let total = 12 + 8 + json.len() + 8 + buffer.bytes.len();
	let mut file = Vec::with_capacity(total);
	file.extend_from_slice(&MAGIC.to_le_bytes());
	file.extend_from_slice(&2_u32.to_le_bytes());
	file.extend_from_slice(&u32::try_from(total)?.to_le_bytes());
	for (kind, chunk) in [(JSON_CHUNK, &json), (BINARY_CHUNK, &buffer.bytes)] {
		file.extend_from_slice(&u32::try_from(chunk.len())?.to_le_bytes());
		file.extend_from_slice(&kind.to_le_bytes());
		file.extend_from_slice(chunk);
	}
	Ok(file)
}

#[cfg(test)]
mod tests {
	use glam::Vec2;

	use super::*;
	use crate::models::geometry::{Vertex, tests::vertex};

	fn word(bytes: &[u8], at: usize) -> u32 {
		u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
	}

	/// Split a GLB into its JSON and its binary chunk, and check the framing on the way.
	fn chunks(file: &[u8]) -> (Value, &[u8]) {
		assert_eq!(word(file, 0), MAGIC, "the magic is `glTF`");
		assert_eq!(word(file, 4), 2, "the version is 2");
		assert_eq!(
			word(file, 8) as usize,
			file.len(),
			"the header holds the length of the file"
		);
		let json_length = word(file, 12) as usize;
		assert_eq!(word(file, 16), JSON_CHUNK, "the JSON chunk comes first");
		assert_eq!(json_length % 4, 0, "the JSON chunk is padded to 4 bytes");
		let json = serde_json::from_slice(&file[20..20 + json_length]).unwrap();
		let at = 20 + json_length;
		let binary_length = word(file, at) as usize;
		assert_eq!(word(file, at + 4), BINARY_CHUNK, "the binary chunk follows");
		assert_eq!(binary_length % 4, 0, "the binary chunk is padded to 4 bytes");
		assert_eq!(at + 8 + binary_length, file.len(), "the chunks fill the file");
		(json, &file[at + 8..])
	}

	fn two_triangles() -> Vec<Triangle> {
		vec![
			[vertex(0.0, 0.0, 0.0), vertex(1.0, 0.0, 0.0), vertex(1.0, 1.0, 0.0)],
			[vertex(0.0, 0.0, 0.0), vertex(1.0, 1.0, 0.0), vertex(0.0, 1.0, 2.0)],
		]
	}

	fn all_parts() -> Vec<(Part, Vec<Triangle>)> {
		Part::ALL.map(|part| (part, two_triangles())).into()
	}

	#[test]
	fn the_file_has_a_header_and_two_chunks_that_fill_it() {
		let file = encode(&all_parts(), b"PNG bytes").unwrap();
		chunks(&file);
	}

	#[test]
	fn the_meshes_are_named_as_the_parts_and_sit_in_nodes_at_the_origin() {
		let file = encode(&all_parts(), b"png").unwrap();
		let (json, _) = chunks(&file);
		let names: Vec<&str> = json["meshes"]
			.as_array()
			.unwrap()
			.iter()
			.map(|m| m["name"].as_str().unwrap())
			.collect();
		assert_eq!(
			names,
			["Fuselage", "LeftWing", "RightWing", "Tail", "Engine"],
			"the names of the variants of Part"
		);
		for (index, node) in json["nodes"].as_array().unwrap().iter().enumerate() {
			assert_eq!(node["mesh"], index, "one node for each mesh");
			assert!(
				node.get("translation").is_none() && node.get("matrix").is_none(),
				"no transform"
			);
		}
		assert_eq!(
			json["scenes"][0]["nodes"],
			json!([0, 1, 2, 3, 4]),
			"the scene lists every node"
		);
	}

	#[test]
	fn every_buffer_view_starts_on_a_multiple_of_four_bytes_inside_the_buffer() {
		let file = encode(&all_parts(), b"odd").unwrap();
		let (json, binary) = chunks(&file);
		assert_eq!(
			json["buffers"][0]["byteLength"],
			binary.len(),
			"the buffer is the binary chunk"
		);
		for view in json["bufferViews"].as_array().unwrap() {
			let (offset, length) = (
				view["byteOffset"].as_u64().unwrap(),
				view["byteLength"].as_u64().unwrap(),
			);
			assert_eq!(offset % 4, 0, "view at {offset} is aligned");
			assert!(
				offset + length <= binary.len() as u64,
				"view at {offset} lies inside the buffer"
			);
		}
	}

	#[test]
	fn the_position_accessor_has_the_minimum_and_the_maximum_of_the_positions() {
		let file = encode(&all_parts(), b"png").unwrap();
		let (json, _) = chunks(&file);
		let position = json["meshes"][0]["primitives"][0]["attributes"]["POSITION"]
			.as_u64()
			.unwrap() as usize;
		let accessor = &json["accessors"][position];
		assert_eq!(accessor["min"], json!([0.0, 0.0, 0.0]), "the minimum corner");
		assert_eq!(accessor["max"], json!([1.0, 1.0, 2.0]), "the maximum corner");
		assert_eq!(
			accessor["count"], 4,
			"the shared vertices are merged: four vertices for two triangles"
		);
	}

	#[test]
	fn the_texture_is_embedded_and_the_material_uses_it_without_metal() {
		let file = encode(&all_parts(), b"the png").unwrap();
		let (json, binary) = chunks(&file);
		let view = &json["bufferViews"][json["images"][0]["bufferView"].as_u64().unwrap() as usize];
		let start = view["byteOffset"].as_u64().unwrap() as usize;
		assert_eq!(
			&binary[start..start + 7],
			b"the png",
			"the image bytes are in the buffer"
		);
		assert_eq!(json["images"][0]["mimeType"], "image/png");
		let material = &json["materials"][0]["pbrMetallicRoughness"];
		assert_eq!(
			material["baseColorTexture"]["index"], 0,
			"the base colour is the texture"
		);
		assert_eq!(material["metallicFactor"], 0.0, "no metal");
		assert!(material["roughnessFactor"].as_f64().unwrap() >= 0.8, "a high roughness");
		assert_eq!(
			json["materials"].as_array().unwrap().len(),
			1,
			"one material for all parts"
		);
	}

	#[test]
	fn the_texture_coordinates_are_turned_to_the_origin_at_the_top() {
		let mut triangle = two_triangles();
		triangle[0][0] = Vertex {
			uv: Vec2::new(0.25, 0.75),
			..triangle[0][0]
		};
		let file = encode(&[(Part::Tail, triangle)], b"png").unwrap();
		let (json, binary) = chunks(&file);
		let uv = json["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_0"]
			.as_u64()
			.unwrap() as usize;
		let view = &json["bufferViews"][json["accessors"][uv]["bufferView"].as_u64().unwrap() as usize];
		let start = view["byteOffset"].as_u64().unwrap() as usize;
		let first = [word(binary, start), word(binary, start + 4)].map(f32::from_bits);
		assert_eq!(first, [0.25, 0.25], "v 0.75 from the bottom is 0.25 from the top");
	}

	#[test]
	fn a_part_without_triangles_is_an_error() {
		let error = encode(&[(Part::Engine, Vec::new())], b"png").unwrap_err();
		assert!(
			error.to_string().contains("Engine"),
			"the message names the part: {error}"
		);
	}
}

//! The meshes of an FBX file as triangles in one frame.
//!
//! The reader applies the transform of each model node and the axis settings of the file. It
//! supports translation, rotation with pre and post rotation, scaling, and the geometric
//! transform. It refuses pivots and offsets, because a file that uses them needs an exporter
//! setting that bakes them in.

use std::collections::HashMap;

use anyhow::{Context, Result, bail, ensure};
use glam::{DMat3, DMat4, DQuat, DVec3, Vec2};

use super::{
	fbx::{Node, Property},
	geometry::{Triangle, Vertex},
};

/// Return the triangles of every mesh in the file, in a frame with +X to the right, +Y up, and +Z toward the viewer.
///
/// The frame is the one that the axis settings of the file name. For the default settings of FBX,
/// the points are only transformed by the nodes.
///
/// # Errors
///
/// Returns an error if the file has no mesh, if a mesh lacks normals or texture coordinates, or if
/// the file uses a feature that the reader does not support.
pub fn read(nodes: &[Node]) -> Result<Vec<Triangle>> {
	let find = |name: &str| nodes.iter().find(|node| node.name == name);
	let objects = find("Objects").context("the file has no `Objects` node")?;
	let links = links(find("Connections"));
	let models: HashMap<i64, &Node> = objects
		.children_named("Model")
		.filter_map(|node| Some((node.properties.first()?.as_i64()?, node)))
		.collect();
	let to_frame = find("GlobalSettings").map_or(Ok(DMat3::IDENTITY), axes)?;
	let mut triangles = Vec::new();
	for geometry in objects.children_named("Geometry") {
		let id = geometry
			.properties
			.first()
			.and_then(Property::as_i64)
			.context("a geometry has no id")?;
		let Some((&model_id, &model)) = links.get(&id).and_then(|parent| models.get_key_value(parent)) else {
			continue;
		};
		let world = world_transform(model_id, &models, &links)? * geometric_transform(model)?;
		triangles.extend(mesh_triangles(geometry, world, to_frame)?);
	}
	ensure!(!triangles.is_empty(), "the file holds no mesh that a model node uses");
	Ok(triangles)
}

/// Map each object to its parent, from the `OO` connections.
fn links(connections: Option<&Node>) -> HashMap<i64, i64> {
	connections
		.into_iter()
		.flat_map(|node| node.children_named("C"))
		.filter(|connection| connection.properties.first().and_then(Property::as_str) == Some("OO"))
		.filter_map(|connection| {
			let child = connection.properties.get(1)?.as_i64()?;
			let parent = connection.properties.get(2)?.as_i64()?;
			Some((child, parent))
		})
		.collect()
}

/// Return the rotation from the axes of the file to +X right, +Y up, +Z toward the viewer.
fn axes(settings: &Node) -> Result<DMat3> {
	let setting = |name: &str, default: i64| -> i64 {
		properties70(settings)
			.find(|(key, _)| *key == name)
			.and_then(|(_, values)| values.first()?.as_i64())
			.unwrap_or(default)
	};
	let axis = |axis: &str, sign: &str, default: i64| -> Result<DVec3> {
		let index = setting(axis, default);
		let sign = setting(sign, 1);
		ensure!(
			(0..3).contains(&index) && sign.abs() == 1,
			"the axis settings of the file are out of range"
		);
		let mut vector = DVec3::ZERO;
		vector[usize::try_from(index)?] = sign as f64;
		Ok(vector)
	};
	let right = axis("CoordAxis", "CoordAxisSign", 0)?;
	let up = axis("UpAxis", "UpAxisSign", 1)?;
	let toward = axis("FrontAxis", "FrontAxisSign", 2)?;
	let to_frame = DMat3::from_cols(right, up, toward).transpose();
	ensure!(
		(to_frame.determinant() - 1.0).abs() < 1e-9,
		"the axis settings of the file make a mirrored frame, which the reader does not support"
	);
	Ok(to_frame)
}

/// Iterate over the `P` entries of the `Properties70` child: the name and the values.
fn properties70(node: &Node) -> impl Iterator<Item = (&str, &[Property])> {
	node.child("Properties70")
		.into_iter()
		.flat_map(|list| list.children_named("P"))
		.filter_map(|entry| Some((entry.properties.first()?.as_str()?, entry.properties.get(4..)?)))
}

fn vector(node: &Node, name: &str, default: DVec3) -> DVec3 {
	properties70(node)
		.find(|(key, _)| *key == name)
		.and_then(|(_, values)| {
			let value = |index: usize| values.get(index)?.as_f64();
			Some(DVec3::new(value(0)?, value(1)?, value(2)?))
		})
		.unwrap_or(default)
}

fn rotation(degrees: DVec3, order: [usize; 3]) -> DQuat {
	let radians = degrees * (std::f64::consts::PI / 180.0);
	let turn = |axis: usize| DQuat::from_axis_angle(DVec3::AXES[axis], radians[axis]);
	// The first axis of the order turns first, so its quaternion is the rightmost factor.
	turn(order[2]) * turn(order[1]) * turn(order[0])
}

/// Return the rotation order of a node as the axes in the order that they turn.
fn rotation_order(node: &Node) -> Result<[usize; 3]> {
	let code = properties70(node)
		.find(|(key, _)| *key == "RotationOrder")
		.and_then(|(_, values)| values.first()?.as_i64())
		.unwrap_or(0);
	Ok(match code {
		0 => [0, 1, 2],
		1 => [0, 2, 1],
		2 => [1, 2, 0],
		3 => [1, 0, 2],
		4 => [2, 0, 1],
		5 => [2, 1, 0],
		other => bail!("the rotation order {other} is not supported"),
	})
}

fn local_transform(model: &Node) -> Result<DMat4> {
	for name in ["RotationOffset", "RotationPivot", "ScalingOffset", "ScalingPivot"] {
		ensure!(
			vector(model, name, DVec3::ZERO) == DVec3::ZERO,
			"the model uses {name}; export the file with the transforms applied"
		);
	}
	let order = rotation_order(model)?;
	let pre = rotation(vector(model, "PreRotation", DVec3::ZERO), order);
	let own = rotation(vector(model, "Lcl Rotation", DVec3::ZERO), order);
	let post = rotation(vector(model, "PostRotation", DVec3::ZERO), order);
	Ok(DMat4::from_scale_rotation_translation(
		vector(model, "Lcl Scaling", DVec3::ONE),
		pre * own * post.inverse(),
		vector(model, "Lcl Translation", DVec3::ZERO),
	))
}

/// The transform of the geometry inside its node, which does not move the children of the node.
fn geometric_transform(model: &Node) -> Result<DMat4> {
	let order = rotation_order(model)?;
	Ok(DMat4::from_scale_rotation_translation(
		vector(model, "GeometricScaling", DVec3::ONE),
		rotation(vector(model, "GeometricRotation", DVec3::ZERO), order),
		vector(model, "GeometricTranslation", DVec3::ZERO),
	))
}

fn world_transform(id: i64, models: &HashMap<i64, &Node>, links: &HashMap<i64, i64>) -> Result<DMat4> {
	let mut world = DMat4::IDENTITY;
	let mut current = id;
	// A chain cannot be longer than the number of models, so a loop in the file ends the walk.
	for _ in 0..=models.len() {
		let Some(model) = models.get(&current) else {
			return Ok(world);
		};
		world = local_transform(model)? * world;
		match links.get(&current) {
			Some(&parent) => current = parent,
			None => return Ok(world),
		}
	}
	bail!("the model nodes form a loop")
}

/// How a layer maps its values to the points of a mesh.
#[derive(Clone, Copy)]
enum Mapping {
	PolygonVertex,
	ControlPoint,
	Polygon,
	AllSame,
}

struct Layer {
	mapping: Mapping,
	values: Vec<f64>,
	/// The index into `values`, if the layer stores indices.
	indices: Option<Vec<i32>>,
}

impl Layer {
	fn read(geometry: &Node, layer_name: &str, values_name: &str, index_name: &str) -> Result<Layer> {
		let layer = geometry.child(layer_name).with_context(|| {
			format!("the mesh has no `{layer_name}`; export it with normals and texture coordinates")
		})?;
		let text = |name: &str| layer.child(name).and_then(|node| node.properties.first()?.as_str());
		let mapping = match text("MappingInformationType") {
			Some("ByPolygonVertex") => Mapping::PolygonVertex,
			Some("ByVertice" | "ByVertex") => Mapping::ControlPoint,
			Some("ByPolygon") => Mapping::Polygon,
			Some("AllSame") => Mapping::AllSame,
			other => bail!("`{layer_name}` has the mapping {other:?}, which the reader does not support"),
		};
		let values = match layer.child(values_name).and_then(|node| node.properties.first()) {
			Some(Property::F64s(values)) => values.clone(),
			Some(Property::F32s(values)) => values.iter().map(|&value| f64::from(value)).collect(),
			_ => bail!("`{layer_name}` has no `{values_name}` array"),
		};
		let indices = match layer.child(index_name).and_then(|node| node.properties.first()) {
			Some(Property::I32s(indices)) => Some(indices.clone()),
			_ => None,
		};
		Ok(Layer {
			mapping,
			values,
			indices,
		})
	}

	/// Return the `width` numbers of the corner `corner` of polygon `polygon`, which uses the control point `point`.
	fn at(&self, corner: usize, point: usize, polygon: usize, width: usize) -> Result<&[f64]> {
		let slot = match self.mapping {
			Mapping::PolygonVertex => corner,
			Mapping::ControlPoint => point,
			Mapping::Polygon => polygon,
			Mapping::AllSame => 0,
		};
		let slot = match &self.indices {
			Some(indices) => usize::try_from(*indices.get(slot).context("a layer index is missing")?)?,
			None => slot,
		};
		self.values
			.get(slot * width..(slot + 1) * width)
			.context("a layer value is missing")
	}
}

fn mesh_triangles(geometry: &Node, world: DMat4, to_frame: DMat3) -> Result<Vec<Triangle>> {
	let property = |name: &str| geometry.child(name).and_then(|node| node.properties.first());
	let Some(Property::F64s(points)) = property("Vertices") else {
		bail!("a mesh has no `Vertices` array");
	};
	let Some(Property::I32s(corners)) = property("PolygonVertexIndex") else {
		bail!("a mesh has no `PolygonVertexIndex` array");
	};
	let normals = Layer::read(geometry, "LayerElementNormal", "Normals", "NormalsIndex")?;
	let uvs = Layer::read(geometry, "LayerElementUV", "UV", "UVIndex")?;
	let linear = DMat3::from_mat4(world);
	let normal_matrix = linear.inverse().transpose();
	let flips = linear.determinant() < 0.0;
	let mut triangles = Vec::new();
	let mut polygon: Vec<Vertex> = Vec::new();
	let mut polygon_index = 0;
	for (corner, &raw) in corners.iter().enumerate() {
		// The last corner of a polygon is stored as its bitwise complement.
		let last = raw < 0;
		let point = usize::try_from(if last { !raw } else { raw })?;
		let position = points
			.get(point * 3..point * 3 + 3)
			.context("a polygon uses a point that does not exist")?;
		let position = world.transform_point3(DVec3::new(position[0], position[1], position[2]));
		let normal = normals.at(corner, point, polygon_index, 3)?;
		let normal = normal_matrix * DVec3::new(normal[0], normal[1], normal[2]);
		let uv = uvs.at(corner, point, polygon_index, 2)?;
		polygon.push(Vertex {
			position: (to_frame * position).as_vec3(),
			normal: (to_frame * normal).normalize_or_zero().as_vec3(),
			uv: Vec2::new(uv[0] as f32, uv[1] as f32),
		});
		if last {
			for [a, b, c] in triangulate(polygon.len()) {
				let triangle = [polygon[a], polygon[b], polygon[c]];
				triangles.push(if flips {
					[triangle[0], triangle[2], triangle[1]]
				} else {
					triangle
				});
			}
			polygon.clear();
			polygon_index += 1;
		}
	}
	ensure!(polygon.is_empty(), "the last polygon of a mesh is not closed");
	Ok(triangles)
}

/// Return the triangles that fill a convex polygon of `corners` corners, as indices into its corners.
///
/// A polygon with fewer than three corners gives no triangle.
fn triangulate(corners: usize) -> Vec<[usize; 3]> {
	(2..corners).map(|next| [0, next - 1, next]).collect()
}

#[cfg(test)]
mod tests {
	use glam::Vec3;

	use super::{
		super::fbx::{
			self,
			tests::{doubles, encode_node, file, int, ints, text},
		},
		*,
	};

	fn long(value: i64) -> Vec<u8> {
		[&b"L"[..], &value.to_le_bytes()].concat()
	}

	fn real(value: f64) -> Vec<u8> {
		[&b"D"[..], &value.to_le_bytes()].concat()
	}

	fn setting(out: &mut Vec<u8>, name: &str, values: &[Vec<u8>]) {
		let mut properties = vec![text(name), text(""), text(""), text("")];
		properties.extend_from_slice(values);
		encode_node(out, "P", &properties, &[]);
	}

	fn layer(out: &mut Vec<u8>, name: &str, values_name: &str, values: &[f64], mapping: &str) {
		encode_node(
			out,
			name,
			&[int(0)],
			&[
				&|out| encode_node(out, "MappingInformationType", &[text(mapping)], &[]),
				&|out| encode_node(out, values_name, &[doubles(values, true)], &[]),
			],
		);
	}

	/// A file with one quad in the XY plane, a model that turns it by -90 degrees about X and scales it by 100.
	fn quad_file(axis_settings: &[(&str, i32)]) -> Vec<u8> {
		file(|out| {
			encode_node(
				out,
				"GlobalSettings",
				&[],
				&[&|out| {
					encode_node(
						out,
						"Properties70",
						&[],
						&[&|out| {
							for (name, value) in axis_settings {
								setting(out, name, &[int(*value)]);
							}
						}],
					);
				}],
			);
			encode_node(
				out,
				"Objects",
				&[],
				&[
					&|out| {
						encode_node(
							out,
							"Geometry",
							&[long(1), text("quad\0\u{1}Geometry"), text("Mesh")],
							&[
								&|out| {
									let corners = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0];
									encode_node(out, "Vertices", &[doubles(&corners, true)], &[]);
								},
								&|out| encode_node(out, "PolygonVertexIndex", &[ints(&[0, 1, 2, -4], true)], &[]),
								&|out| layer(out, "LayerElementNormal", "Normals", &[0.0, 0.0, 1.0], "AllSame"),
								&|out| {
									layer(
										out,
										"LayerElementUV",
										"UV",
										&[0.0, 0.0, 0.5, 0.0, 0.5, 0.5, 0.0, 0.5],
										"ByVertice",
									)
								},
							],
						);
					},
					&|out| {
						encode_node(
							out,
							"Model",
							&[long(2), text("quad\0\u{1}Model"), text("Mesh")],
							&[&|out| {
								encode_node(
									out,
									"Properties70",
									&[],
									&[&|out| {
										setting(out, "Lcl Rotation", &[real(-90.0), real(0.0), real(0.0)]);
										setting(out, "Lcl Scaling", &[real(100.0), real(100.0), real(100.0)]);
									}],
								);
							}],
						);
					},
				],
			);
			encode_node(
				out,
				"Connections",
				&[],
				&[
					&|out| encode_node(out, "C", &[text("OO"), long(2), long(0)], &[]),
					&|out| encode_node(out, "C", &[text("OO"), long(1), long(2)], &[]),
				],
			);
		})
	}

	fn read_quad(axis_settings: &[(&str, i32)]) -> Vec<Triangle> {
		read(&fbx::parse(&quad_file(axis_settings)).unwrap()).unwrap()
	}

	#[test]
	fn a_quad_becomes_two_triangles() {
		assert_eq!(read_quad(&[]).len(), 2, "a polygon of four corners has two triangles");
	}

	#[test]
	fn a_polygon_of_n_corners_gives_n_minus_two_triangles() {
		assert_eq!(triangulate(2), Vec::<[usize; 3]>::new(), "two corners are no polygon");
		assert_eq!(triangulate(3), [[0, 1, 2]], "a triangle stays one");
		assert_eq!(triangulate(5).len(), 3, "a pentagon has three triangles");
	}

	#[test]
	fn the_rotation_and_the_scale_of_the_model_node_move_the_points() {
		let triangles = read_quad(&[]);
		let corner = triangles
			.iter()
			.flatten()
			.find(|v| v.uv == Vec2::new(0.5, 0.5))
			.unwrap();
		assert!(
			corner.position.abs_diff_eq(Vec3::new(100.0, 0.0, -100.0), 1e-3),
			"(1, 1, 0) scaled by 100 and turned by -90 degrees about X: {}",
			corner.position
		);
		assert!(
			corner.normal.abs_diff_eq(Vec3::Y, 1e-6),
			"the normal turns with the surface: {}",
			corner.normal
		);
	}

	#[test]
	fn the_axis_settings_of_the_file_turn_a_z_up_file_to_y_up() {
		let z_up = [("UpAxis", 2), ("FrontAxis", 1), ("FrontAxisSign", -1), ("CoordAxis", 0)];
		let triangles = read_quad(&z_up);
		let corner = triangles
			.iter()
			.flatten()
			.find(|v| v.uv == Vec2::new(0.5, 0.5))
			.unwrap();
		// Before the axes, the corner is at (100, 0, -100): the file Z is up, so it is at height -100.
		assert!(
			corner.position.abs_diff_eq(Vec3::new(100.0, -100.0, 0.0), 1e-3),
			"the file Z becomes Y, and the file -Y becomes Z: {}",
			corner.position
		);
	}

	#[test]
	fn a_file_without_a_mesh_is_refused() {
		let bytes = file(|out| encode_node(out, "Objects", &[], &[]));
		assert!(read(&fbx::parse(&bytes).unwrap()).is_err(), "no mesh is an error");
	}
}

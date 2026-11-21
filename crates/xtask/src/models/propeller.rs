//! Take the blades of a propeller out of a plane model, so that the client can draw a blur in their place.

use std::collections::HashMap;

use anyhow::{Result, ensure};
use glam::{Vec2, Vec3};
use serde::Deserialize;

use crate::mesh::geometry::{Triangle, area};

/// The size of the grid that joins the corners of triangles into pieces, in metres.
const WELD: f32 = 1e-4;

/// The stretch of the nose in which the blades of a model turn, in metres ahead of the middle of the plane.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Stretch {
	/// Where the stretch starts.
	pub from: f32,
	/// Where the stretch ends, further ahead.
	pub to: f32,
}

/// The disc that the blades of a propeller sweep, in the frame of the plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Disc {
	/// The middle of the disc, on the axis of the propeller, in metres.
	pub centre: Vec3,
	/// The reach of the longest blade from the axis, in metres.
	pub radius: f32,
}

/// Return the number of the piece of each triangle. Two triangles that share a corner are one piece.
fn pieces(triangles: &[Triangle]) -> Vec<usize> {
	fn root(parents: &mut [usize], mut node: usize) -> usize {
		while parents[node] != node {
			parents[node] = parents[parents[node]];
			node = parents[node];
		}
		node
	}
	let mut corners: HashMap<[i32; 3], usize> = HashMap::new();
	let mut parents: Vec<usize> = (0..triangles.len()).collect();
	for (index, triangle) in triangles.iter().enumerate() {
		for vertex in triangle {
			let corner = (vertex.position / WELD).round().as_ivec3().to_array();
			let first = *corners.entry(corner).or_insert(index);
			let (a, b) = (root(&mut parents, first), root(&mut parents, index));
			parents[a] = b;
		}
	}
	(0..triangles.len()).map(|index| root(&mut parents, index)).collect()
}

/// Remove the blades of the propeller from `triangles`, and return the disc that they sweep.
///
/// A blade is a piece of the model that shares no corner with the rest and lies wholly in `stretch`.
/// The hub and the engine reach behind the stretch, so they stay.
///
/// # Errors
///
/// Returns an error if no piece of the model lies wholly in the stretch.
pub fn remove_blades(triangles: &mut Vec<Triangle>, stretch: Stretch) -> Result<Disc> {
	let piece_of = pieces(triangles);
	// The nose points to -Z, so a place ahead of the middle has a negative Z.
	let mut reach: HashMap<usize, (f32, f32)> = HashMap::new();
	for (triangle, piece) in triangles.iter().zip(&piece_of) {
		let (low, high) = reach.entry(*piece).or_insert((f32::MAX, f32::MIN));
		for vertex in triangle {
			*low = low.min(-vertex.position.z);
			*high = high.max(-vertex.position.z);
		}
	}
	let is_blade = |piece: &usize| reach[piece].0 >= stretch.from && reach[piece].1 <= stretch.to;
	let blades: Vec<Triangle> = triangles
		.iter()
		.zip(&piece_of)
		.filter(|(_, piece)| is_blade(piece))
		.map(|(triangle, _)| *triangle)
		.collect();
	let swept: f32 = blades.iter().map(area).sum();
	ensure!(
		swept > 0.0,
		"no separate piece of the model lies between {} m and {} m ahead of the middle. \
		 Read the place of the blades from the preview of `cargo xtask models --preview`, and correct `propeller` in planes.toml",
		stretch.from,
		stretch.to
	);
	// The blades stand evenly around the axis, so the middle of their surface is on the axis.
	let centre = blades
		.iter()
		.map(|triangle| triangle.iter().map(|vertex| vertex.position).sum::<Vec3>() / 3.0 * area(triangle))
		.sum::<Vec3>()
		/ swept;
	let radius = blades
		.iter()
		.flatten()
		.map(|vertex| Vec2::new(vertex.position.x - centre.x, vertex.position.y - centre.y).length())
		.fold(0.0, f32::max);
	let mut piece = piece_of.iter();
	triangles.retain(|_| !piece.next().is_some_and(is_blade));
	Ok(Disc { centre, radius })
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::mesh::geometry::tests::vertex;

	/// A flat blade that reaches `length` metres from the axis to each side along X, `ahead` metres ahead of the middle.
	fn blade(ahead: f32, length: f32) -> Vec<Triangle> {
		let corner = |x: f32, y: f32| vertex(x, 0.5 + y, -ahead);
		vec![
			[corner(-length, -0.1), corner(length, -0.1), corner(length, 0.1)],
			[corner(-length, -0.1), corner(length, 0.1), corner(-length, 0.1)],
		]
	}

	/// A hub that starts 1 m ahead of the middle and ends `ahead` metres ahead.
	fn hub(ahead: f32) -> Vec<Triangle> {
		vec![[
			vertex(-0.1, 0.5, -1.0),
			vertex(0.1, 0.5, -1.0),
			vertex(0.0, 0.5, -ahead),
		]]
	}

	const STRETCH: Stretch = Stretch { from: 3.5, to: 3.9 };

	#[test]
	fn the_blades_in_the_stretch_leave_the_model_and_give_the_disc_that_they_sweep() {
		let mut model = [hub(4.0), blade(3.7, 1.2)].concat();
		let disc = remove_blades(&mut model, STRETCH).unwrap();
		assert_eq!(model, hub(4.0), "the hub stays, and the blade is gone");
		assert!(
			disc.centre.distance(Vec3::new(0.0, 0.5, -3.7)) < 1e-4,
			"the disc is on the axis of the blade: {}",
			disc.centre
		);
		assert!(
			(disc.radius - Vec2::new(1.2, 0.1).length()).abs() < 1e-4,
			"the disc reaches the tip of the blade: {}",
			disc.radius
		);
	}

	#[test]
	fn a_piece_that_reaches_out_of_the_stretch_stays() {
		let mut model = [hub(3.7), blade(3.7, 1.2), blade(3.95, 0.5)].concat();
		remove_blades(&mut model, STRETCH).unwrap();
		assert_eq!(
			model,
			[hub(3.7), blade(3.95, 0.5)].concat(),
			"the hub starts behind the stretch, and the far piece ends ahead of it"
		);
	}

	#[test]
	fn a_piece_that_shares_a_corner_with_a_longer_piece_stays_with_it() {
		let fin = vec![[vertex(1.2, 0.4, -3.7), vertex(1.2, 0.6, -3.7), vertex(1.2, 0.5, -1.0)]];
		let mut model = [blade(3.7, 1.2), fin].concat();
		let message = remove_blades(&mut model, STRETCH).unwrap_err().to_string();
		assert!(
			message.contains("planes.toml"),
			"a blade that is joined to a piece behind the stretch is no blade: {message}"
		);
		assert_eq!(model.len(), 3, "the model stays whole after the error");
	}

	#[test]
	fn a_stretch_without_a_piece_is_an_error_with_a_hint() {
		let mut model = hub(4.0);
		let message = remove_blades(&mut model, STRETCH).unwrap_err().to_string();
		assert!(
			message.contains("3.5") && message.contains("--preview"),
			"the message names the stretch and the way to find the right one: {message}"
		);
	}
}

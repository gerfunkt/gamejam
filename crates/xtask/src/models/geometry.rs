//! Triangles with normals and texture coordinates, and the clipping of a triangle at a plane.

use glam::{Vec2, Vec3};

/// A point of a surface with the data that the renderer needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
	/// The position, in metres.
	pub position: Vec3,
	/// The unit vector that is perpendicular to the surface.
	pub normal: Vec3,
	/// The texture coordinates, with the origin at the bottom left, as in FBX.
	pub uv: Vec2,
}

impl Vertex {
	fn lerp(self, other: Vertex, t: f32) -> Vertex {
		Vertex {
			position: self.position.lerp(other.position, t),
			normal: self.normal.lerp(other.normal, t).try_normalize().unwrap_or(self.normal),
			uv: self.uv.lerp(other.uv, t),
		}
	}
}

/// Three vertices in counter-clockwise order, seen from the side that the normal points to.
pub type Triangle = [Vertex; 3];

/// Return the area of `triangle`, in square metres.
#[must_use]
pub fn area(triangle: &Triangle) -> f32 {
	let [a, b, c] = triangle.map(|vertex| vertex.position);
	(b - a).cross(c - a).length() / 2.0
}

/// One of the three coordinate axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
	/// The X axis.
	X,
	/// The Y axis.
	Y,
	/// The Z axis.
	Z,
}

impl Axis {
	/// Return the coordinate of `point` along this axis.
	#[must_use]
	pub fn of(self, point: Vec3) -> f32 {
		match self {
			Axis::X => point.x,
			Axis::Y => point.y,
			Axis::Z => point.z,
		}
	}

	fn set(self, point: &mut Vec3, value: f32) {
		match self {
			Axis::X => point.x = value,
			Axis::Y => point.y = value,
			Axis::Z => point.z = value,
		}
	}
}

/// The plane where one coordinate has a fixed value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
	/// The axis that is perpendicular to the plane.
	pub axis: Axis,
	/// The coordinate of the plane along that axis, in metres.
	pub offset: f32,
}

/// Cut `triangle` at `plane`, and return the triangles below it and the triangles on or above it.
///
/// The pieces together cover the same area as `triangle`. Their new vertices lie exactly on the plane,
/// and carry interpolated normals and texture coordinates. A piece without area is dropped.
#[must_use]
pub fn clip(triangle: &Triangle, plane: Plane) -> (Vec<Triangle>, Vec<Triangle>) {
	let below = |vertex: &Vertex| plane.axis.of(vertex.position) < plane.offset;
	let (mut low, mut high) = (Vec::new(), Vec::new());
	for (index, &from) in triangle.iter().enumerate() {
		let to = triangle[(index + 1) % 3];
		if below(&from) {
			low.push(from);
		} else {
			high.push(from);
		}
		if below(&from) != below(&to) {
			let start = plane.axis.of(from.position);
			let end = plane.axis.of(to.position);
			let mut crossing = from.lerp(to, (plane.offset - start) / (end - start));
			plane.axis.set(&mut crossing.position, plane.offset);
			low.push(crossing);
			high.push(crossing);
		}
	}
	(fan(&low), fan(&high))
}

fn fan(polygon: &[Vertex]) -> Vec<Triangle> {
	polygon
		.windows(2)
		.skip(1)
		.map(|pair| [polygon[0], pair[0], pair[1]])
		.filter(|triangle| area(triangle) > 1e-9)
		.collect()
}

/// Return the smallest box around the triangles, as its minimum and maximum corners.
///
/// The box of no triangles is `None`.
#[must_use]
pub fn bounds(triangles: &[Triangle]) -> Option<(Vec3, Vec3)> {
	triangles
		.iter()
		.flatten()
		.map(|vertex| vertex.position)
		.fold(None, |bounds, point| match bounds {
			None => Some((point, point)),
			Some((low, high)) => Some((low.min(point), high.max(point))),
		})
}

#[cfg(test)]
pub(crate) mod tests {
	use super::*;

	pub(crate) fn vertex(x: f32, y: f32, z: f32) -> Vertex {
		Vertex {
			position: Vec3::new(x, y, z),
			normal: Vec3::Z,
			uv: Vec2::new(x, y),
		}
	}

	fn total(triangles: &[Triangle]) -> f32 {
		triangles.iter().map(area).sum()
	}

	const PLANE: Plane = Plane {
		axis: Axis::X,
		offset: 1.0,
	};

	#[test]
	fn a_triangle_on_one_side_of_the_plane_is_not_cut() {
		let triangle = [vertex(2.0, 0.0, 0.0), vertex(3.0, 0.0, 0.0), vertex(2.0, 1.0, 0.0)];
		let (low, high) = clip(&triangle, PLANE);
		assert!(low.is_empty(), "nothing lies below the plane");
		assert_eq!(high, [triangle], "the triangle stays whole above it");
	}

	#[test]
	fn the_pieces_of_a_cut_triangle_keep_its_area_and_lie_on_their_sides() {
		for triangle in [
			[vertex(0.0, 0.0, 0.0), vertex(3.0, 0.0, 0.0), vertex(0.0, 2.0, 0.0)],
			[vertex(0.0, 0.0, 0.0), vertex(3.0, 0.0, 0.0), vertex(2.0, 2.0, 1.0)],
			[vertex(0.5, 0.0, 0.0), vertex(2.0, 3.0, 0.0), vertex(0.0, 1.0, 2.0)],
		] {
			let (low, high) = clip(&triangle, PLANE);
			let whole = total(&[triangle]);
			assert!(
				(total(&low) + total(&high) - whole).abs() < 1e-5 * whole,
				"the areas of the pieces add up to {whole}"
			);
			assert!(
				low.iter().flatten().all(|v| v.position.x <= 1.0 + 1e-6),
				"below the plane"
			);
			assert!(
				high.iter().flatten().all(|v| v.position.x >= 1.0 - 1e-6),
				"above the plane"
			);
			assert!(!low.is_empty() && !high.is_empty(), "both sides get a piece");
		}
	}

	#[test]
	fn a_cut_vertex_has_the_interpolated_texture_coordinates_and_a_unit_normal() {
		let mut a = vertex(0.0, 0.0, 0.0);
		let mut b = vertex(2.0, 0.0, 0.0);
		a.normal = Vec3::X;
		b.normal = Vec3::Y;
		a.uv = Vec2::new(0.0, 0.0);
		b.uv = Vec2::new(1.0, 0.5);
		let (low, _) = clip(&[a, b, vertex(0.0, 2.0, 0.0)], PLANE);
		let cut = low
			.iter()
			.flatten()
			.find(|v| v.position.x == 1.0 && v.position.y == 0.0)
			.expect("the edge from a to b is cut at its middle");
		assert_eq!(cut.uv, Vec2::new(0.5, 0.25), "halfway between the texture coordinates");
		assert!((cut.normal.length() - 1.0).abs() < 1e-6, "the normal has unit length");
		assert!(
			(cut.normal.x - cut.normal.y).abs() < 1e-6,
			"the normal is halfway between X and Y"
		);
	}

	#[test]
	fn a_triangle_that_touches_the_plane_with_one_vertex_gives_no_empty_sliver() {
		let triangle = [vertex(0.0, 0.0, 0.0), vertex(1.0, 1.0, 0.0), vertex(2.0, 0.0, 0.0)];
		let (low, high) = clip(&triangle, PLANE);
		assert_eq!(low.len(), 1, "one piece below");
		assert_eq!(high.len(), 1, "one piece above");
	}

	#[test]
	fn the_bounds_of_no_triangles_are_none() {
		assert_eq!(bounds(&[]), None, "an empty list has no box");
	}
}

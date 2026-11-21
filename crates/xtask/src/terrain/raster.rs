//! The view from above: the highest surface of a mesh over each point of a square lattice.

use std::collections::VecDeque;

use anyhow::{Result, ensure};
use glam::{DVec2, Vec2};

use crate::mesh::geometry::{Triangle, Vertex};

/// A weight below this, in the parts of a triangle, still counts as inside it. It closes the cracks on shared edges.
const EDGE: f64 = -1e-9;

/// Points in rows and columns at equal distances, over a square around the origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lattice {
	origin: f64,
	step: f64,
	count: usize,
}

impl Lattice {
	/// Return the lattice of `count` points on a side whose first and last points lie on the edges of the square.
	#[must_use]
	pub fn corners(side: f32, count: usize) -> Self {
		let side = f64::from(side);
		Self {
			origin: -side / 2.0,
			step: side / (count - 1) as f64,
			count,
		}
	}

	/// Return the lattice of `count` points on a side that lie in the middles of `count` equal cells of the square.
	#[must_use]
	pub fn centres(side: f32, count: usize) -> Self {
		let side = f64::from(side);
		let step = side / count as f64;
		Self {
			origin: -side / 2.0 + step / 2.0,
			step,
			count,
		}
	}

	fn coordinate(self, index: usize) -> f64 {
		self.origin + index as f64 * self.step
	}

	/// Return the indices of the points whose coordinate lies between `low` and `high`.
	fn between(self, low: f64, high: f64) -> std::ops::Range<usize> {
		let first = ((low - self.origin) / self.step).ceil().max(0.0);
		let last = ((high - self.origin) / self.step).floor().min((self.count - 1) as f64);
		if first > last {
			0..0
		} else {
			first as usize..last as usize + 1
		}
	}
}

/// The top surface at one point of a lattice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
	/// The height of the surface, in the units of the mesh.
	pub height: f32,
	/// The texture coordinates of the surface.
	pub uv: Vec2,
}

const NONE: Hit = Hit {
	height: f32::NEG_INFINITY,
	uv: Vec2::ZERO,
};

/// Return the highest surface of `triangles` over each point of `lattice`, row by row with the lowest Z first.
///
/// A point that no triangle covers takes the hit of the nearest covered point along the rows and
/// the columns of the lattice.
///
/// # Errors
///
/// Returns an error if no triangle covers any point of the lattice.
pub fn top_surface(triangles: &[Triangle], lattice: Lattice) -> Result<Vec<Hit>> {
	let mut hits = vec![NONE; lattice.count * lattice.count];
	for triangle in triangles {
		let [a, b, c] = triangle.map(|vertex| {
			(
				DVec2::new(f64::from(vertex.position.x), f64::from(vertex.position.z)),
				vertex,
			)
		});
		let area = (b.0 - a.0).perp_dot(c.0 - a.0);
		if area.abs() < 1e-12 {
			continue;
		}
		let (low, high) = (a.0.min(b.0).min(c.0), a.0.max(b.0).max(c.0));
		for row in lattice.between(low.y, high.y) {
			for column in lattice.between(low.x, high.x) {
				let point = DVec2::new(lattice.coordinate(column), lattice.coordinate(row));
				let weights = [
					(b.0 - point).perp_dot(c.0 - point) / area,
					(c.0 - point).perp_dot(a.0 - point) / area,
					(a.0 - point).perp_dot(b.0 - point) / area,
				];
				if weights.iter().any(|&weight| weight < EDGE) {
					continue;
				}
				let mix = |value: fn(Vertex) -> f32| {
					(weights[0] * f64::from(value(a.1))
						+ weights[1] * f64::from(value(b.1))
						+ weights[2] * f64::from(value(c.1))) as f32
				};
				let height = mix(|vertex| vertex.position.y);
				let slot = &mut hits[row * lattice.count + column];
				if height > slot.height {
					*slot = Hit {
						height,
						uv: Vec2::new(mix(|vertex| vertex.uv.x), mix(|vertex| vertex.uv.y)),
					};
				}
			}
		}
	}
	fill(&mut hits, lattice.count)?;
	Ok(hits)
}

/// Give each point without a hit the hit of the nearest point with one, by spreading the hits one step at a time.
fn fill(hits: &mut [Hit], count: usize) -> Result<()> {
	let mut queue: VecDeque<usize> = (0..hits.len())
		.filter(|&slot| hits[slot].height > f32::NEG_INFINITY)
		.collect();
	ensure!(
		!queue.is_empty(),
		"the model covers no point of the grid; check the axes of the model in terrain.toml"
	);
	while let Some(slot) = queue.pop_front() {
		let (column, row) = (slot % count, slot / count);
		let neighbours = [
			(column > 0).then(|| slot - 1),
			(column + 1 < count).then(|| slot + 1),
			(row > 0).then(|| slot - count),
			(row + 1 < count).then(|| slot + count),
		];
		for neighbour in neighbours.into_iter().flatten() {
			if hits[neighbour].height == f32::NEG_INFINITY {
				hits[neighbour] = hits[slot];
				queue.push_back(neighbour);
			}
		}
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use glam::Vec2;

	use super::*;
	use crate::mesh::geometry::tests::vertex;

	fn triangle(corners: [[f32; 3]; 3]) -> Triangle {
		corners.map(|[x, y, z]| vertex(x, y, z))
	}

	/// The square lattice of 5 points on a side from -100 to 100.
	fn lattice() -> Lattice {
		Lattice::corners(200.0, 5)
	}

	#[test]
	fn the_corner_lattice_has_its_first_and_last_points_on_the_edges() {
		let lattice = lattice();
		assert_eq!(
			(lattice.coordinate(0), lattice.coordinate(4)),
			(-100.0, 100.0),
			"on the edges"
		);
		assert_eq!(lattice.coordinate(2), 0.0, "the middle point is at the origin");
	}

	#[test]
	fn the_centre_lattice_has_one_point_in_the_middle_of_each_cell() {
		let lattice = Lattice::centres(200.0, 4);
		assert_eq!(
			(lattice.coordinate(0), lattice.coordinate(3)),
			(-75.0, 75.0),
			"a quarter of a side from each edge"
		);
	}

	#[test]
	fn a_flat_triangle_gives_its_height_to_the_points_under_it() {
		let flat = triangle([[-100.0, 7.0, -100.0], [100.0, 7.0, -100.0], [-100.0, 7.0, 100.0]]);
		let hits = top_surface(&[flat], lattice()).unwrap();
		assert_eq!(hits[0].height, 7.0, "the corner point under the triangle");
		assert_eq!(hits[2].height, 7.0, "a point on its edge");
	}

	#[test]
	fn the_highest_surface_over_a_point_wins() {
		let low = triangle([[-100.0, 1.0, -100.0], [100.0, 1.0, -100.0], [-100.0, 1.0, 100.0]]);
		let high = triangle([[-100.0, 9.0, -100.0], [100.0, 9.0, -100.0], [-100.0, 9.0, 100.0]]);
		for order in [[low, high], [high, low]] {
			assert_eq!(
				top_surface(&order, lattice()).unwrap()[0].height,
				9.0,
				"the order does not matter"
			);
		}
	}

	#[test]
	fn a_tilted_triangle_gives_the_interpolated_height_and_texture_coordinates() {
		let mut slope = triangle([[-100.0, 0.0, -100.0], [100.0, 20.0, -100.0], [-100.0, 0.0, 100.0]]);
		slope[0].uv = Vec2::new(0.0, 0.0);
		slope[1].uv = Vec2::new(1.0, 0.0);
		slope[2].uv = Vec2::new(0.0, 1.0);
		let hits = top_surface(&[slope], lattice()).unwrap();
		assert!(
			(hits[1].height - 5.0).abs() < 1e-4,
			"a quarter of the way along X: {}",
			hits[1].height
		);
		assert!(
			(hits[1].uv - Vec2::new(0.25, 0.0)).length() < 1e-5,
			"the same quarter in u: {}",
			hits[1].uv
		);
	}

	#[test]
	fn a_pyramid_has_its_peak_over_the_middle_point_and_falls_toward_the_edges() {
		let apex = [0.0, 50.0, 0.0];
		let corners = [
			[-100.0, 0.0, -100.0],
			[100.0, 0.0, -100.0],
			[100.0, 0.0, 100.0],
			[-100.0, 0.0, 100.0],
		];
		let sides: Vec<Triangle> = (0..4)
			.map(|side| triangle([corners[side], corners[(side + 1) % 4], apex]))
			.collect();
		let hits = top_surface(&sides, lattice()).unwrap();
		let heights: Vec<f32> = hits.iter().map(|hit| hit.height).collect();
		let peak = heights.iter().copied().fold(f32::MIN, f32::max);
		assert_eq!(peak, 50.0, "the apex height");
		assert_eq!(heights[2 * 5 + 2], 50.0, "the peak is over the middle point");
		assert_eq!(
			heights.iter().filter(|&&height| height == peak).count(),
			1,
			"no other point is as high"
		);
		assert!(
			(heights[2 * 5 + 3] - 25.0).abs() < 1e-3,
			"halfway down the slope: {}",
			heights[2 * 5 + 3]
		);
		assert_eq!(heights[0], 0.0, "the corner of the base");
	}

	#[test]
	fn a_point_that_no_triangle_covers_takes_the_hit_of_the_nearest_covered_point() {
		let left = triangle([[-100.0, 3.0, -100.0], [-40.0, 3.0, -100.0], [-100.0, 3.0, 100.0]]);
		let hits = top_surface(&[left], lattice()).unwrap();
		assert_eq!(hits[0].height, 3.0, "a covered point");
		assert_eq!(
			hits[4].height, 3.0,
			"the far edge takes the height of the nearest covered point in its row"
		);
		assert!(hits.iter().all(|hit| hit.height == 3.0), "nothing stays empty");
	}

	#[test]
	fn a_mesh_that_covers_no_point_is_an_error_with_a_hint() {
		let away = triangle([[500.0, 0.0, 500.0], [600.0, 0.0, 500.0], [500.0, 0.0, 600.0]]);
		let message = top_surface(&[away], lattice()).unwrap_err().to_string();
		assert!(
			message.contains("terrain.toml"),
			"the message names the file to check: {message}"
		);
	}

	#[test]
	fn a_wall_without_width_covers_nothing() {
		let wall = triangle([[0.0, 0.0, 0.0], [0.0, 90.0, 0.0], [0.0, 0.0, 50.0]]);
		let flat = triangle([[-100.0, 2.0, -100.0], [100.0, 2.0, -100.0], [-100.0, 2.0, 100.0]]);
		assert_eq!(
			top_surface(&[wall, flat], lattice()).unwrap()[2 * 5 + 2].height,
			2.0,
			"a wall is no ground"
		);
	}
}

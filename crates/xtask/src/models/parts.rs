//! Cut a plane model into the five parts that damage breaks.

use sim::Part;

use super::geometry::{Axis, Plane, Triangle, clip};

/// The planes that separate the parts of the rules from each other.
///
/// The engine is the part of the plane before its box, and the tail is the part behind its box.
/// Between them, what lies outside the fuselage box to the left or the right belongs to a wing.
/// The cuts along Z come first, because the propeller and the tailplane are wider than the fuselage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Regions {
	left_wing_x: f32,
	right_wing_x: f32,
	engine_z: f32,
	tail_z: f32,
}

impl Regions {
	/// Return the regions that the boxes of `sim::Part` give.
	#[must_use]
	pub fn of_rules() -> Self {
		let fuselage = (Part::Fuselage.centre(), Part::Fuselage.half_extents());
		let engine = (Part::Engine.centre(), Part::Engine.half_extents());
		let tail = (Part::Tail.centre(), Part::Tail.half_extents());
		Self {
			left_wing_x: fuselage.0.x - fuselage.1.x,
			right_wing_x: fuselage.0.x + fuselage.1.x,
			engine_z: engine.0.z + engine.1.z,
			tail_z: tail.0.z - tail.1.z,
		}
	}

	/// Cut the triangles at the planes of the regions, and return the pieces of each part, in the order of `Part::ALL`.
	#[must_use]
	pub fn split(&self, triangles: &[Triangle]) -> Vec<(Part, Vec<Triangle>)> {
		let along = |axis, offset| Plane { axis, offset };
		let mut rest = triangles.to_vec();
		let mut pieces = Vec::new();
		// Each cut takes one side off `rest` for a part. `low` says which side that is.
		for (part, plane, low) in [
			(Part::Engine, along(Axis::Z, self.engine_z), true),
			(Part::Tail, along(Axis::Z, self.tail_z), false),
			(Part::LeftWing, along(Axis::X, self.left_wing_x), true),
			(Part::RightWing, along(Axis::X, self.right_wing_x), false),
		] {
			let (below, above) = cut_all(&rest, plane);
			let (taken, kept) = if low { (below, above) } else { (above, below) };
			pieces.push((part, taken));
			rest = kept;
		}
		pieces.push((Part::Fuselage, rest));
		pieces.sort_by_key(|(part, _)| Part::ALL.iter().position(|other| other == part));
		pieces
	}
}

fn cut_all(triangles: &[Triangle], plane: Plane) -> (Vec<Triangle>, Vec<Triangle>) {
	let (mut below, mut above) = (Vec::new(), Vec::new());
	for triangle in triangles {
		let (low, high) = clip(triangle, plane);
		below.extend(low);
		above.extend(high);
	}
	(below, above)
}

#[cfg(test)]
mod tests {
	use glam::Vec3;

	use super::*;
	use crate::models::geometry::{area, tests::vertex};

	impl Regions {
		/// Return the part that owns `point`, by the same rules as the cuts.
		pub fn part_at(&self, point: Vec3) -> Part {
			if point.z < self.engine_z {
				Part::Engine
			} else if point.z >= self.tail_z {
				Part::Tail
			} else if point.x < self.left_wing_x {
				Part::LeftWing
			} else if point.x >= self.right_wing_x {
				Part::RightWing
			} else {
				Part::Fuselage
			}
		}
	}

	fn centroid(triangle: &Triangle) -> Vec3 {
		triangle.iter().map(|vertex| vertex.position).sum::<Vec3>() / 3.0
	}

	/// Two large triangles that cover the box of the plane, and reach beyond it on every side.
	fn sheet() -> Vec<Triangle> {
		let corner = |x, z| vertex(x, 0.2, z);
		vec![
			[corner(-8.0, -7.0), corner(8.0, -7.0), corner(8.0, 6.0)],
			[corner(-8.0, -7.0), corner(8.0, 6.0), corner(-8.0, 6.0)],
		]
	}

	#[test]
	fn the_regions_follow_the_boxes_of_the_rules() {
		let regions = Regions::of_rules();
		assert_eq!(
			regions.part_at(Vec3::new(0.0, 0.0, -3.5)),
			Part::Engine,
			"in front of the fuselage"
		);
		assert_eq!(
			regions.part_at(Vec3::new(0.0, 0.5, 3.5)),
			Part::Tail,
			"behind the fuselage"
		);
		assert_eq!(
			regions.part_at(Vec3::new(-3.0, 0.0, 0.3)),
			Part::LeftWing,
			"left of the fuselage"
		);
		assert_eq!(
			regions.part_at(Vec3::new(3.0, 0.0, 0.3)),
			Part::RightWing,
			"right of the fuselage"
		);
		assert_eq!(
			regions.part_at(Vec3::new(0.2, 0.0, 0.0)),
			Part::Fuselage,
			"inside the fuselage"
		);
		assert_eq!(
			regions.part_at(Vec3::new(1.4, 0.5, 3.3)),
			Part::Tail,
			"the tailplane is wider than the fuselage"
		);
	}

	#[test]
	fn the_pieces_of_a_split_keep_the_area_and_lie_in_their_regions() {
		let regions = Regions::of_rules();
		let sheet = sheet();
		let pieces = regions.split(&sheet);
		let parts: Vec<Part> = pieces.iter().map(|(part, _)| *part).collect();
		assert_eq!(parts, Part::ALL, "one entry for each part, in the order of Part::ALL");
		let before: f32 = sheet.iter().map(area).sum();
		let after: f32 = pieces.iter().flat_map(|(_, triangles)| triangles).map(area).sum();
		assert!(
			(before - after).abs() < 1e-3 * before,
			"the cuts keep the area: {before} and {after}"
		);
		for (part, triangles) in &pieces {
			assert!(!triangles.is_empty(), "the sheet reaches {part:?}");
			for triangle in triangles {
				assert_eq!(
					regions.part_at(centroid(triangle)),
					*part,
					"a piece of {part:?} lies in its region"
				);
			}
		}
	}
}

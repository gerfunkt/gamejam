//! Cut a plane model into the five parts that damage breaks.

use sim::Part;

use super::geometry::{Axis, Plane, Triangle, clip};

/// The z coordinate in the frame of the plane, in metres, from which a piece beside the fuselage belongs to the tail.
///
/// It lies behind the middle of the plane with a margin, so that the whole tailplane, which is wide,
/// goes with the tail, and not with a wing.
const WING_END_Z: f32 = 1.5;
/// How far below the box of the fuselage the undercarriage starts, in metres.
///
/// The lower wing of a biplane reaches a little below the box, so the margin keeps it with the wings.
const UNDERCARRIAGE_MARGIN: f32 = 0.25;

/// The planes that separate the parts of the rules from each other.
///
/// - The engine is everything before the box of the engine, whatever its width. The propeller is wider than the fuselage.
/// - The tail is everything behind the box of the tail. Beside the fuselage, it also starts at [`WING_END_Z`].
/// - A piece beside the fuselage box, ahead of that, belongs to a wing. But a piece far below the box is
///   the undercarriage, which belongs to the fuselage, so that wheels and struts do not split between the wings.
///   This rule is by height, because the undercarriage is as long in Z as the wing is wide.
/// - The rest is the fuselage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Regions {
	left_wing_x: f32,
	right_wing_x: f32,
	engine_z: f32,
	tail_z: f32,
	undercarriage_y: f32,
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
			undercarriage_y: fuselage.0.y - fuselage.1.y - UNDERCARRIAGE_MARGIN,
		}
	}

	/// Cut the triangles at the planes of the regions, and return the pieces of each part, in the order of `Part::ALL`.
	#[must_use]
	pub fn split(&self, triangles: &[Triangle]) -> Vec<(Part, Vec<Triangle>)> {
		let along = |axis, offset| Plane { axis, offset };
		let (engine, rest) = cut_all(triangles, along(Axis::Z, self.engine_z));
		let (left, rest) = cut_all(&rest, along(Axis::X, self.left_wing_x));
		let (rest, right) = cut_all(&rest, along(Axis::X, self.right_wing_x));
		let (mut fuselage, mut tail) = cut_all(&rest, along(Axis::Z, self.tail_z));
		let [left_wing, right_wing] = [left, right].map(|side| {
			let (ahead, behind) = cut_all(&side, along(Axis::Z, WING_END_Z));
			tail.extend(behind);
			let (undercarriage, wing) = cut_all(&ahead, along(Axis::Y, self.undercarriage_y));
			fuselage.extend(undercarriage);
			wing
		});
		let mut pieces = vec![
			(Part::Engine, engine),
			(Part::Tail, tail),
			(Part::Fuselage, fuselage),
			(Part::LeftWing, left_wing),
			(Part::RightWing, right_wing),
		];
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
			let beside = point.x < self.left_wing_x || point.x >= self.right_wing_x;
			if point.z < self.engine_z {
				Part::Engine
			} else if point.z >= self.tail_z || (beside && point.z >= WING_END_Z) {
				Part::Tail
			} else if beside && point.y >= self.undercarriage_y {
				if point.x < 0.0 { Part::LeftWing } else { Part::RightWing }
			} else {
				Part::Fuselage
			}
		}
	}

	fn centroid(triangle: &Triangle) -> Vec3 {
		triangle.iter().map(|vertex| vertex.position).sum::<Vec3>() / 3.0
	}

	/// Two large triangles at height `y` that cover the box of the plane, and reach beyond it on every side.
	fn sheet(y: f32) -> Vec<Triangle> {
		let corner = |x, z| vertex(x, y, z);
		vec![
			[corner(-8.0, -7.0), corner(8.0, -7.0), corner(8.0, 6.0)],
			[corner(-8.0, -7.0), corner(8.0, 6.0), corner(-8.0, 6.0)],
		]
	}

	/// Return the one part that holds a small triangle around `(x, y, z)`.
	fn owner(x: f32, y: f32, z: f32) -> Part {
		let corner = |dx, dz| vertex(x + dx, y, z + dz);
		let pieces = Regions::of_rules().split(&[[corner(-0.05, 0.0), corner(0.05, 0.0), corner(0.0, 0.05)]]);
		let owners: Vec<Part> = pieces
			.iter()
			.filter(|(_, t)| !t.is_empty())
			.map(|(part, _)| *part)
			.collect();
		assert_eq!(
			owners.len(),
			1,
			"a small triangle at ({x}, {y}, {z}) stays whole: {owners:?}"
		);
		owners[0]
	}

	#[test]
	fn a_wide_piece_behind_the_middle_of_the_plane_belongs_to_the_tail_not_to_a_wing() {
		assert_eq!(owner(-1.4, 0.0, 2.0), Part::Tail, "the front of the tailplane, left");
		assert_eq!(owner(1.4, 0.0, 2.0), Part::Tail, "the front of the tailplane, right");
		assert_eq!(
			owner(-3.0, 0.0, 1.0),
			Part::LeftWing,
			"ahead of the margin it is still a wing"
		);
		assert_eq!(
			owner(0.0, 0.0, 2.0),
			Part::Fuselage,
			"inside the fuselage box it stays the fuselage"
		);
	}

	#[test]
	fn a_piece_far_below_the_fuselage_box_is_the_undercarriage_and_belongs_to_the_fuselage() {
		assert_eq!(owner(-1.5, -1.5, -2.0), Part::Fuselage, "a wheel left of the fuselage");
		assert_eq!(owner(1.5, -1.5, -2.0), Part::Fuselage, "a wheel right of the fuselage");
		assert_eq!(
			owner(-1.5, -1.5, -3.0),
			Part::Engine,
			"ahead of the engine cut, the engine keeps it"
		);
		assert_eq!(
			owner(-1.5, -1.5, 2.0),
			Part::Tail,
			"behind the margin, the tail keeps it"
		);
	}

	#[test]
	fn the_lower_wing_of_a_biplane_stays_with_the_wings() {
		assert_eq!(
			owner(-3.0, -0.6, -2.0),
			Part::LeftWing,
			"a lower wing a little below the box"
		);
		assert_eq!(owner(3.0, -0.7, -2.0), Part::RightWing, "the same on the right");
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
		for y in [0.2, -1.5] {
			check_sheet(&regions, y);
		}
	}

	fn check_sheet(regions: &Regions, y: f32) {
		let sheet = sheet(y);
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
			let wing = matches!(part, Part::LeftWing | Part::RightWing);
			assert!(wing && y < 0.0 || !triangles.is_empty(), "the sheet reaches {part:?}");
			assert!(
				!(wing && y < 0.0) || triangles.is_empty(),
				"no wing hangs below the undercarriage line"
			);
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

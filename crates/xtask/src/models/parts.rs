//! Cut a plane model into the five parts that damage breaks.

use sim::Part;

use super::geometry::{Axis, Plane, Triangle, bounds, clip};

/// How far past the fuselage box a piece must reach to count as a wing, as a part of the way to the widest point of the model.
///
/// Small parts beside the fuselage, such as engine nacelles and struts, stay below it.
const WING_REACH: f32 = 0.25;
/// How far below the box of the fuselage the undercarriage starts, in metres.
///
/// The lower wing of a biplane reaches a little below the box, so the margin keeps it with the wings.
const UNDERCARRIAGE_MARGIN: f32 = 0.25;

/// The planes that separate the parts of the rules from each other.
///
/// - The engine is everything before the box of the engine, whatever its width. The propeller is wider than the fuselage.
/// - The tail is everything behind the box of the tail. Beside the fuselage, it also starts at the end of the wing.
///   [`Regions::for_model`] finds that end in the model, and [`Regions::with_wing_end`] sets it.
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
	wing_end_z: f32,
	undercarriage_y: f32,
}

impl Regions {
	/// Return the regions that the boxes of `sim::Part` give, with the wing running to the box of the tail.
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
			wing_end_z: tail.0.z - tail.1.z,
			undercarriage_y: fuselage.0.y - fuselage.1.y - UNDERCARRIAGE_MARGIN,
		}
	}

	/// Return these regions with the wing ending at `wing_end_z`, in metres.
	#[must_use]
	pub fn with_wing_end(self, wing_end_z: f32) -> Self {
		Self { wing_end_z, ..self }
	}

	/// Return the regions of the rules, with the end of the wing taken from the shape of `triangles`.
	///
	/// The wing is the run of Z, between the engine and the tail, in which the model reaches well past the
	/// fuselage box: at least [`WING_REACH`] of the way from the box to its widest point. A run that
	/// starts later, such as a wide tailplane, is not the wing. The wing ends where its run ends.
	/// The undercarriage does not count, and a model with nothing beside the fuselage keeps the regions of the rules.
	#[must_use]
	pub fn for_model(triangles: &[Triangle]) -> Self {
		let rules = Self::of_rules();
		let Some((low, high)) = bounds(triangles) else {
			return rules;
		};
		let along = |axis, offset| Plane { axis, offset };
		let reach = rules.right_wing_x + WING_REACH * (low.x.abs().max(high.x) - rules.right_wing_x);
		let (_, rest) = cut_all(triangles, along(Axis::Z, rules.engine_z));
		let (rest, _) = cut_all(&rest, along(Axis::Z, rules.tail_z));
		let (_, rest) = cut_all(&rest, along(Axis::Y, rules.undercarriage_y));
		let (left, _) = cut_all(&rest, along(Axis::X, -reach));
		let (_, right) = cut_all(&rest, along(Axis::X, reach));
		let outer: Vec<Triangle> = left.into_iter().chain(right).collect();
		let widest = outer
			.iter()
			.flatten()
			.max_by(|a, b| a.position.x.abs().total_cmp(&b.position.x.abs()))
			.map(|vertex| vertex.position.z);
		let Some(widest) = widest else {
			return rules;
		};
		let mut spans: Vec<(f32, f32)> = outer
			.iter()
			.map(|triangle| {
				let z = triangle.map(|vertex| vertex.position.z);
				(z[0].min(z[1]).min(z[2]), z[0].max(z[1]).max(z[2]))
			})
			.collect();
		spans.sort_by(|a, b| a.0.total_cmp(&b.0));
		let mut runs: Vec<(f32, f32)> = Vec::new();
		for (start, end) in spans {
			match runs.last_mut() {
				Some(run) if start <= run.1 => run.1 = run.1.max(end),
				_ => runs.push((start, end)),
			}
		}
		let wing_end = runs
			.iter()
			.find(|run| run.0 <= widest && widest <= run.1)
			.map_or(rules.wing_end_z, |run| run.1);
		rules.with_wing_end(wing_end)
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
			let (ahead, behind) = cut_all(&side, along(Axis::Z, self.wing_end_z));
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
			} else if point.z >= self.tail_z || (beside && point.z >= self.wing_end_z) {
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

	/// The regions of the rules, with the wing ending 1.5 m behind the middle of the plane.
	fn regions() -> Regions {
		Regions::of_rules().with_wing_end(1.5)
	}

	/// Return the one part that holds a small triangle around `(x, y, z)`.
	fn owner(x: f32, y: f32, z: f32) -> Part {
		let corner = |dx, dz| vertex(x + dx, y, z + dz);
		let pieces = regions().split(&[[corner(-0.05, 0.0), corner(0.05, 0.0), corner(0.0, 0.05)]]);
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
		let regions = regions();
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
		let regions = regions();
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

	/// Both sides of a flat piece of the plan view of the plane, as a polygon of `(x, z)` corners on the left side, at height 0.
	fn planform(corners: &[(f32, f32)]) -> Vec<Triangle> {
		[1.0, -1.0]
			.into_iter()
			.flat_map(|side| {
				let corner = move |index: usize| vertex(corners[index].0 * side, 0.0, corners[index].1);
				(1..corners.len() - 1).map(move |index| [corner(0), corner(index), corner(index + 1)])
			})
			.collect()
	}

	/// A tailplane at the rear, joined to the wing by thin nacelles beside the fuselage.
	fn tail_and_nacelles() -> Vec<Triangle> {
		let tailplane = planform(&[(0.5, 2.0), (2.5, 2.4), (2.5, 2.8), (0.5, 2.8)]);
		let nacelles = planform(&[(0.7, 0.5), (0.8, 0.5), (0.8, 2.0), (0.7, 2.0)]);
		[tailplane, nacelles].concat()
	}

	#[test]
	fn the_wing_ends_where_the_widest_run_beside_the_fuselage_ends_and_not_at_a_tailplane() {
		let swept_back = planform(&[(0.6, -1.0), (5.0, 0.2), (5.0, 0.8), (0.6, 0.0)]);
		let triangles = [swept_back, tail_and_nacelles()].concat();
		let regions = Regions::for_model(&triangles);
		assert_eq!(
			regions.part_at(Vec3::new(-4.0, 0.0, 0.7)),
			Part::LeftWing,
			"the trailing tip is wing"
		);
		assert_eq!(
			regions.part_at(Vec3::new(2.0, 0.0, 2.3)),
			Part::Tail,
			"the tailplane is tail"
		);
		assert_eq!(
			regions.part_at(Vec3::new(0.75, 0.0, 1.5)),
			Part::Tail,
			"a nacelle behind the wing is tail"
		);
	}

	#[test]
	fn a_wing_whose_root_trails_behind_its_tip_stays_whole() {
		let swept_forward = planform(&[(0.6, -1.0), (5.0, -1.0), (5.0, -0.4), (0.6, 1.0)]);
		let triangles = [swept_forward, tail_and_nacelles()].concat();
		let regions = Regions::for_model(&triangles);
		assert_eq!(
			regions.part_at(Vec3::new(-1.0, 0.0, 0.6)),
			Part::LeftWing,
			"the root of the wing runs behind the tip and is still wing"
		);
		assert_eq!(
			regions.part_at(Vec3::new(2.0, 0.0, 2.3)),
			Part::Tail,
			"the tailplane is tail"
		);
	}

	#[test]
	fn a_model_without_anything_beside_the_fuselage_keeps_the_regions_of_the_rules() {
		let narrow = planform(&[(0.0, -1.0), (0.5, -1.0), (0.5, 1.0), (0.0, 1.0)]);
		assert_eq!(Regions::for_model(&narrow), Regions::of_rules(), "no wing to measure");
		assert_eq!(Regions::for_model(&[]), Regions::of_rules(), "no triangles");
	}
}

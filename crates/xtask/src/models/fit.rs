//! Turn a model to the frame of the plane of the rules, and scale it to the size of that plane.

use std::str::FromStr;

use anyhow::{Result, bail, ensure};
use glam::Vec3;
use serde::Deserialize;
use sim::Part;

use super::geometry::{Axis, Triangle, bounds};

/// An axis of a model with a direction, written as `+X`, `-Z`, and so on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct SignedAxis {
	axis: Axis,
	positive: bool,
}

impl SignedAxis {
	fn direction(self) -> Vec3 {
		let unit = match self.axis {
			Axis::X => Vec3::X,
			Axis::Y => Vec3::Y,
			Axis::Z => Vec3::Z,
		};
		if self.positive { unit } else { -unit }
	}
}

impl FromStr for SignedAxis {
	type Err = anyhow::Error;

	fn from_str(text: &str) -> Result<Self> {
		let axis = match text.get(1..) {
			Some("X" | "x") => Axis::X,
			Some("Y" | "y") => Axis::Y,
			Some("Z" | "z") => Axis::Z,
			_ => bail!("`{text}` is not an axis; write +X, -X, +Y, -Y, +Z, or -Z"),
		};
		match text.get(..1) {
			Some("+") => Ok(SignedAxis { axis, positive: true }),
			Some("-") => Ok(SignedAxis { axis, positive: false }),
			_ => bail!("`{text}` is not an axis; write +X, -X, +Y, -Y, +Z, or -Z"),
		}
	}
}

impl TryFrom<String> for SignedAxis {
	type Error = anyhow::Error;

	fn try_from(text: String) -> Result<Self> {
		text.parse()
	}
}

/// Which axes of a model point to the nose and to the sky.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Orientation {
	/// The axis along which the plane flies.
	pub nose: SignedAxis,
	/// The axis that points up from the plane.
	pub up: SignedAxis,
}

/// Turn the triangles so that the nose points to -Z and up is +Y.
///
/// The turn is a rotation, so it keeps the winding of the triangles.
///
/// # Errors
///
/// Returns an error if the nose and the up axis are the same line.
pub fn orient(triangles: &mut [Triangle], orientation: Orientation) -> Result<()> {
	let (nose, up) = (orientation.nose.direction(), orientation.up.direction());
	ensure!(
		nose.dot(up) == 0.0,
		"the nose and the up axis of a model must be two different axes"
	);
	// The right of a plane is its nose crossed with its up direction.
	let right = nose.cross(up);
	let turn = |vector: Vec3| Vec3::new(vector.dot(right), vector.dot(up), -vector.dot(nose));
	for vertex in triangles.iter_mut().flatten() {
		vertex.position = turn(vertex.position);
		vertex.normal = turn(vertex.normal);
	}
	Ok(())
}

/// Return the box that holds all five parts of the plane of the rules, as its minimum and maximum corners.
///
/// Its X extent is the wing span, and its Z extent is the length from the engine to the tail.
#[must_use]
pub fn rules_box() -> (Vec3, Vec3) {
	Part::ALL.iter().fold((Vec3::MAX, Vec3::MIN), |(low, high), part| {
		(
			low.min(part.centre() - part.half_extents()),
			high.max(part.centre() + part.half_extents()),
		)
	})
}

/// Scale the triangles uniformly, and move them, so that they fill `target` in X and Z.
///
/// The scale is the largest one under which the span fits the X extent and the length fits the Z extent.
/// The model keeps its proportions. Its centre goes to the centre of `target` in X and Z, and to 0 in Y,
/// where the rules put the centre of the fuselage. The function returns the scale.
///
/// # Errors
///
/// Returns an error if the model has no extent in X or in Z.
pub fn fit(triangles: &mut [Triangle], target: (Vec3, Vec3)) -> Result<f32> {
	let Some((low, high)) = bounds(triangles) else {
		bail!("the model has no triangles");
	};
	let (size, target_size) = (high - low, target.1 - target.0);
	ensure!(
		size.x > 0.0 && size.z > 0.0,
		"the model is flat; check the axes of the model in planes.toml"
	);
	let scale = (target_size.x / size.x).min(target_size.z / size.z);
	let centre = (low + high) / 2.0;
	let target_centre = (target.0 + target.1) / 2.0;
	let offset = Vec3::new(target_centre.x, 0.0, target_centre.z) - centre * scale;
	for vertex in triangles.iter_mut().flatten() {
		vertex.position = vertex.position * scale + offset;
	}
	Ok(scale)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::models::geometry::tests::vertex;

	fn triangle(corners: [[f32; 3]; 3]) -> Triangle {
		corners.map(|[x, y, z]| vertex(x, y, z))
	}

	#[test]
	fn an_axis_is_written_with_a_sign_and_a_letter() {
		assert_eq!("-Z".parse::<SignedAxis>().unwrap().direction(), -Vec3::Z, "-Z");
		assert_eq!("+X".parse::<SignedAxis>().unwrap().direction(), Vec3::X, "+X");
		assert!("Z".parse::<SignedAxis>().is_err(), "a sign is needed");
		assert!("+W".parse::<SignedAxis>().is_err(), "W is no axis");
	}

	#[test]
	fn a_model_with_its_nose_along_x_and_up_along_z_is_turned_to_the_frame_of_the_plane() {
		let mut triangles = [triangle([[10.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 5.0]])];
		let orientation = Orientation {
			nose: "+X".parse().unwrap(),
			up: "+Z".parse().unwrap(),
		};
		orient(&mut triangles, orientation).unwrap();
		let [nose, side, sky] = triangles[0].map(|vertex| vertex.position);
		assert_eq!(nose, Vec3::new(0.0, 0.0, -10.0), "the nose point goes to -Z");
		assert_eq!(sky, Vec3::new(0.0, 5.0, 0.0), "the up point goes to +Y");
		// Nose +X crossed with up +Z is -Y, so the model -Y is the right side, which is +X in the plane.
		assert_eq!(
			side,
			Vec3::new(-1.0, 0.0, 0.0),
			"the model +Y is the left side, which is -X"
		);
	}

	#[test]
	fn a_nose_and_an_up_axis_on_one_line_are_refused() {
		let orientation = Orientation {
			nose: "+X".parse().unwrap(),
			up: "-X".parse().unwrap(),
		};
		assert!(orient(&mut [], orientation).is_err(), "the axes must differ");
	}

	#[test]
	fn a_long_model_fits_the_length_and_keeps_its_proportions() {
		let target = (Vec3::new(-5.0, -1.0, -4.0), Vec3::new(5.0, 1.0, 3.0));
		let mut triangles = [triangle([[0.0, 0.0, 0.0], [2.0, 1.0, 0.0], [0.0, 3.0, 4.0]])];
		let scale = fit(&mut triangles, target).unwrap();
		let (low, high) = bounds(&triangles).unwrap();
		assert!((scale - 7.0 / 4.0).abs() < 1e-6, "the length limits the scale: {scale}");
		assert!(((high.z - low.z) - 7.0).abs() < 1e-5, "the length is 7 m");
		assert!(
			((high.x - low.x) - 3.5).abs() < 1e-5,
			"the span keeps its ratio to the length"
		);
		assert!(
			((high.y - low.y) - 5.25).abs() < 1e-5,
			"the height keeps its ratio to the length"
		);
		assert!(
			((high.z + low.z) / 2.0 + 0.5).abs() < 1e-5,
			"centred along Z on the target"
		);
		assert!((high.x + low.x).abs() < 1e-5, "centred across X on the target");
		assert!((high.y + low.y).abs() < 1e-5, "centred at height 0");
	}

	#[test]
	fn a_wide_model_fits_the_span() {
		let target = (Vec3::new(-5.0, -1.0, -4.0), Vec3::new(5.0, 1.0, 4.0));
		let mut triangles = [triangle([[0.0, 0.0, 0.0], [40.0, 0.0, 0.0], [0.0, 0.0, 4.0]])];
		fit(&mut triangles, target).unwrap();
		let (low, high) = bounds(&triangles).unwrap();
		assert!(((high.x - low.x) - 10.0).abs() < 1e-5, "the span is 10 m");
		assert!(((high.z - low.z) - 1.0).abs() < 1e-5, "the length is below 8 m");
	}

	#[test]
	fn the_rules_box_is_the_wing_span_wide_and_the_engine_to_tail_long() {
		let (low, high) = rules_box();
		assert!(
			((high.x - low.x) - 11.2).abs() < 1e-5,
			"two wings of 5 m, 0.6 m apart on each side of the centre"
		);
		assert!(
			((high.z - low.z) - 7.9).abs() < 1e-5,
			"from -4.0 at the engine to 3.9 at the tail"
		);
	}
}

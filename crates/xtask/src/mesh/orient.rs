//! Turn a model so that its nose axis points to -Z and its up axis to +Y.

use std::str::FromStr;

use anyhow::{Result, bail, ensure};
use glam::Vec3;
use serde::Deserialize;

use super::geometry::{Axis, Triangle};

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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orientation {
	/// The axis along which the plane flies.
	pub nose: SignedAxis,
	/// The axis that points up from the plane.
	pub up: SignedAxis,
	/// The angle in degrees by which to lift the nose after the axes are turned. A negative angle lowers it.
	pub pitch: f32,
}

/// Turn the triangles so that the nose points to -Z and up is +Y, and then lift the nose by the pitch.
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
	pitch(triangles, orientation.pitch);
	Ok(())
}

/// Turn the triangles about the X axis of the plane, so that a positive angle in degrees lifts the nose.
pub fn pitch(triangles: &mut [Triangle], degrees: f32) {
	let (sin, cos) = degrees.to_radians().sin_cos();
	let lift = |vector: Vec3| {
		Vec3::new(
			vector.x,
			vector.y * cos - vector.z * sin,
			vector.y * sin + vector.z * cos,
		)
	};
	if degrees == 0.0 {
		return;
	}
	for vertex in triangles.iter_mut().flatten() {
		vertex.position = lift(vertex.position);
		vertex.normal = lift(vertex.normal);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::mesh::geometry::tests::vertex;

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
			pitch: 0.0,
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
	fn a_positive_pitch_lifts_the_nose_and_turns_the_normals_with_it() {
		let mut triangles = [triangle([[0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]])];
		pitch(&mut triangles, 90.0);
		let [nose, side, top] = triangles[0].map(|vertex| vertex.position);
		assert!(nose.abs_diff_eq(Vec3::Y, 1e-6), "the nose at -Z goes up: {nose}");
		assert!(side.abs_diff_eq(Vec3::X, 1e-6), "the X axis stays");
		assert!(top.abs_diff_eq(Vec3::Z, 1e-6), "the top goes toward the tail: {top}");
		assert!(
			triangles[0][0].normal.abs_diff_eq(Vec3::NEG_Y, 1e-6),
			"the normal +Z turns to -Y"
		);
	}

	#[test]
	fn a_pitch_of_zero_changes_nothing() {
		let original = [triangle([[0.0, 0.5, -1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]])];
		let mut triangles = original;
		pitch(&mut triangles, 0.0);
		assert_eq!(triangles, original, "no turn");
	}

	#[test]
	fn a_nose_and_an_up_axis_on_one_line_are_refused() {
		let orientation = Orientation {
			nose: "+X".parse().unwrap(),
			up: "-X".parse().unwrap(),
			pitch: 0.0,
		};
		assert!(orient(&mut [], orientation).is_err(), "the axes must differ");
	}
}

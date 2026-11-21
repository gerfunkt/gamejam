use glam::{Quat, Vec2, Vec3};

use crate::tuning::{BANK_FULL_ANGLE, BANK_START_ANGLE, PITCH_RATE, ROLL_FULL_ANGLE, STEER_FULL_ANGLE, YAW_RATE};

/// The deflection of the three control axes, each from -1 to 1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Steering {
	/// Positive raises the nose.
	pub pitch: f32,
	/// Positive turns the nose to the right.
	pub yaw: f32,
	/// Positive lowers the right wing.
	pub roll: f32,
}

/// Return the orientation that looks along the nose of `orientation` with level wings.
///
/// The camera and the aim of a player use this frame, so that a bank of the plane does not turn
/// the view. A nose that points straight up or down has no level frame, and the function then
/// returns `orientation` unchanged.
#[must_use]
pub fn level_frame(orientation: Quat) -> Quat {
	let forward = orientation * Vec3::NEG_Z;
	let right = forward.cross(Vec3::Y);
	if right.length_squared() < 1e-6 {
		return orientation;
	}
	let right = right.normalize();
	let up = right.cross(forward);
	Quat::from_mat3(&glam::Mat3::from_cols(right, up, -forward)).normalize()
}

/// Return the direction in the world that lies `offset` away from the nose of a plane.
///
/// `offset` is the tangent of the angle to the right and of the angle upward, in the level
/// frame of the plane. A zero offset gives the direction of the nose, so a cursor at the centre
/// of the view flies straight.
#[must_use]
pub fn aim_direction(orientation: Quat, offset: Vec2) -> Vec3 {
	level_frame(orientation) * Vec3::new(offset.x, offset.y, -1.0).normalize()
}

/// Return the control deflections that turn a plane toward the direction `aim` in the world.
///
/// Players and bots both steer through this function. The plane banks into a wide turn, because
/// pitch is its strong axis, and it levels its wings when the aim is close to the nose.
#[must_use]
pub fn steer_toward(orientation: Quat, aim: Vec3) -> Steering {
	let local = orientation.inverse() * aim.normalize_or(orientation * Vec3::NEG_Z);
	// The angle between the nose and the aim, and the side of the nose on which the aim lies.
	let side = Vec2::new(local.x, local.y);
	let angle = side.length().atan2(-local.z);
	let side = side.normalize_or(Vec2::Y);
	let deflection = (angle / STEER_FULL_ANGLE).min(1.0);

	// The roll that puts the aim above the nose, where a pull reaches it.
	let into_turn = side.x.atan2(side.y);
	// The roll that levels the wings.
	let right = orientation * Vec3::X;
	let up = orientation * Vec3::Y;
	let to_level = right.y.atan2(up.y);
	let bank = ((angle - BANK_START_ANGLE) / (BANK_FULL_ANGLE - BANK_START_ANGLE)).clamp(0.0, 1.0);
	let roll_error = bank * into_turn + (1.0 - bank) * to_level;

	// The nose must move straight toward the aim. Pitch is stronger than yaw, so at full
	// deflection on both axes the nose would climb in each banked turn. Thus the weaker axis
	// sets the pace until the bank puts the aim above the nose.
	let pace = (PITCH_RATE / side.y.abs().max(1e-6)).min(YAW_RATE / side.x.abs().max(1e-6));
	Steering {
		pitch: deflection * side.y * pace / PITCH_RATE,
		yaw: deflection * side.x * pace / YAW_RATE,
		roll: (roll_error / ROLL_FULL_ANGLE).clamp(-1.0, 1.0),
	}
}

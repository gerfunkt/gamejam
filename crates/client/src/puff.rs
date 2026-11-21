//! The soft puff from which the clouds and the effects are built: a square that always faces the camera.
//!
//! The texture of a puff shows a ball of cloud that the sun lights from above. Each puff turns so
//! that this lit side points toward the sun in the picture. Thus a heap of puffs reads as a lit
//! volume from every side, for the cost of one square each.

use bevy::{
	asset::RenderAssetUsages,
	prelude::*,
	render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use crate::{sky, view::Drawn};

/// The side of each puff texture, in pixels.
const TEXTURE_SIZE: u32 = 128;
/// The brightness of a puff on its side away from the sun, as a part of the lit side.
const SHADE: f32 = 0.62;
/// How far the edge of a puff wavers inward, as a part of its radius.
const RAGGED_EDGE: f32 = 0.45;
/// The number of lumps across the edge of a puff.
const LUMPS: f32 = 3.5;

/// The mesh and the textures that every puff shares.
#[derive(Resource)]
pub struct PuffAssets {
	/// A square of side 1 m that faces +Z.
	pub square: Handle<Mesh>,
	/// A ball of cloud with a ragged edge, lit from the top of the texture.
	pub cloud: Handle<Image>,
	/// A round glow that fades from its centre, for fire and light.
	pub glow: Handle<Image>,
}

/// A square that turns to face the camera, with the top of its texture toward the sun.
#[derive(Component)]
pub struct Billboard;

pub fn plugin(app: &mut App) {
	app.add_systems(PreStartup, make_assets)
		.add_systems(Update, face_camera.after(Drawn));
}

fn make_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut images: ResMut<Assets<Image>>) {
	commands.insert_resource(PuffAssets {
		square: meshes.add(Rectangle::new(1.0, 1.0)),
		cloud: images.add(texture(cloud_texel)),
		glow: images.add(texture(glow_texel)),
	});
}

/// Turn every billboard to face the camera, with its lit side toward the sun.
fn face_camera(
	camera: Single<&Transform, (With<Camera3d>, Without<Billboard>)>,
	mut boards: Query<&mut Transform, With<Billboard>>,
) {
	// The direction of the sun across the picture, with +Y as the top of the picture.
	let across = (camera.rotation.inverse() * sky::sun()).truncate();
	let turn = camera.rotation * Quat::from_rotation_z(f32::atan2(-across.x, across.y));
	for mut transform in &mut boards {
		transform.rotation = turn;
	}
}

/// Return a pseudo-random number from 0 to 1 for each seed, the same on every machine.
pub fn random(seed: u32) -> f32 {
	// The hash of Chris Wellons ("lowbias32").
	let mut x = seed;
	x ^= x >> 16;
	x = x.wrapping_mul(0x7feb_352d);
	x ^= x >> 15;
	x = x.wrapping_mul(0x846c_a68b);
	x ^= x >> 16;
	(x >> 8) as f32 / (1 << 24) as f32
}

/// Return smooth noise from 0 to 1 at a point of the plane, with lumps about 1 apart.
fn noise(point: Vec2) -> f32 {
	let cell = point.floor();
	let along = point - cell;
	let smooth = along * along * (3.0 - 2.0 * along);
	let corner = |dx: i32, dy: i32| {
		let (x, y) = (cell.x as i32 + dx, cell.y as i32 + dy);
		random((x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663))
	};
	let bottom = corner(0, 0) + (corner(1, 0) - corner(0, 0)) * smooth.x;
	let top = corner(0, 1) + (corner(1, 1) - corner(0, 1)) * smooth.x;
	bottom + (top - bottom) * smooth.y
}

/// Return the colour and the cover of the cloud texture at a point from -1 to 1 on each axis.
fn cloud_texel(point: Vec2) -> [f32; 4] {
	let lumps = (noise(point * LUMPS) + 0.5 * noise(point * LUMPS * 2.0 + 17.0)) / 1.5;
	let edge = point.length() + RAGGED_EDGE * (lumps - 0.5);
	let cover = 1.0 - smoothstep(0.35, 1.0, edge);
	// The texture is the picture of a ball, and the sun shines on it from the top and a little from the front.
	let normal = point.extend((1.0 - point.length_squared()).max(0.0).sqrt()).normalize();
	let light = 0.5 + 0.5 * normal.dot(Vec3::new(0.0, 0.85, 0.5).normalize());
	let shade = SHADE + (1.0 - SHADE) * light * (0.9 + 0.1 * lumps);
	[shade, shade, shade, cover * (0.8 + 0.2 * lumps)]
}

/// Return the colour and the cover of the glow texture at a point from -1 to 1 on each axis.
fn glow_texel(point: Vec2) -> [f32; 4] {
	let distance = point.length();
	[
		1.0,
		1.0,
		1.0,
		(-5.0 * distance * distance).exp() * (1.0 - smoothstep(0.8, 1.0, distance)),
	]
}

fn smoothstep(low: f32, high: f32, value: f32) -> f32 {
	let along = ((value - low) / (high - low)).clamp(0.0, 1.0);
	along * along * (3.0 - 2.0 * along)
}

/// Return a square texture whose texels a function gives, with +Y up.
fn texture(texel: fn(Vec2) -> [f32; 4]) -> Image {
	let side = TEXTURE_SIZE as f32;
	let mut data = Vec::with_capacity((TEXTURE_SIZE * TEXTURE_SIZE * 4) as usize);
	for row in 0..TEXTURE_SIZE {
		for column in 0..TEXTURE_SIZE {
			let point = Vec2::new(
				(column as f32 + 0.5) / side * 2.0 - 1.0,
				1.0 - (row as f32 + 0.5) / side * 2.0,
			);
			data.extend(texel(point).map(|value| (value.clamp(0.0, 1.0) * 255.0).round() as u8));
		}
	}
	Image::new(
		Extent3d {
			width: TEXTURE_SIZE,
			height: TEXTURE_SIZE,
			depth_or_array_layers: 1,
		},
		TextureDimension::D2,
		data,
		TextureFormat::Rgba8Unorm,
		RenderAssetUsages::RENDER_WORLD,
	)
}

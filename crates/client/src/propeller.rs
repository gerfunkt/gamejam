//! The blur of a propeller that turns.
//!
//! The build takes the blades out of a plane model. In their place, the client draws a disc with
//! the smears that fast blades leave in the eye, and turns it.

use bevy::{
	asset::RenderAssetUsages,
	prelude::*,
	render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

/// The side of the picture of the blur, in pixels.
const SIDE: usize = 128;
/// The number of smears on the disc.
const SMEARS: f32 = 3.0;
/// How much of the view the disc hides between the smears, from 0 to 1.
const HAZE: f32 = 0.11;
/// How much more a smear hides at its middle.
const SMEAR: f32 = 0.24;
/// How much more the ring of the blade tips hides.
const TIPS: f32 = 0.14;
/// The part of the radius that the hub covers. The disc is clear there, because the model keeps its hub.
const HUB: f32 = 0.1;
/// The colour of the blades, and of the paint on their tips.
const BLADE: [f32; 3] = [28.0, 26.0, 24.0];
const TIP_PAINT: [f32; 3] = [215.0, 180.0, 70.0];
/// How fast the smears turn, in radians for each second. Real blades turn faster than a frame can show.
const TURN_RATE: f32 = 9.0;

/// The mesh and the material that every propeller shares.
#[derive(Resource)]
pub struct Blur {
	mesh: Handle<Mesh>,
	material: Handle<StandardMaterial>,
}

impl Blur {
	/// Return the picture of a propeller that turns at `centre` and reaches `radius` metres, in the frame of its plane.
	pub fn disc(&self, centre: Vec3, radius: f32) -> impl Bundle {
		(
			Turning,
			Mesh3d(self.mesh.clone()),
			MeshMaterial3d(self.material.clone()),
			Transform::from_translation(centre).with_scale(Vec3::splat(radius)),
		)
	}
}

/// A disc that turns about the axis of its propeller.
#[derive(Component)]
struct Turning;

pub fn plugin(app: &mut App) {
	app.add_systems(Startup, make_blur).add_systems(Update, turn);
}

/// Return the picture of the disc: dark smears on a faint haze, a ring at the tips, and nothing at the hub and the rim.
fn picture() -> Image {
	// Return how far `value` is on the way from `from` to `to`, from 0 to 1.
	let ramp = |from: f32, to: f32, value: f32| ((value - from) / (to - from)).clamp(0.0, 1.0);
	let mut data = Vec::with_capacity(SIDE * SIDE * 4);
	for row in 0..SIDE {
		for column in 0..SIDE {
			let point = (Vec2::new(column as f32, row as f32) + 0.5) / SIDE as f32 * 2.0 - 1.0;
			let (radius, angle) = (point.length(), point.to_angle());
			let smear = (0.5 + 0.5 * (SMEARS * angle).cos()).powi(2);
			let tips = ramp(0.84, 0.9, radius) * ramp(0.99, 0.94, radius);
			let inside = ramp(HUB, 1.6 * HUB, radius) * ramp(1.0, 0.95, radius);
			let cover = (HAZE + SMEAR * smear + TIPS * tips) * inside;
			for (blade, paint) in BLADE.iter().zip(TIP_PAINT) {
				data.push((blade + (paint - blade) * tips) as u8);
			}
			data.push((cover * 255.0) as u8);
		}
	}
	Image::new(
		Extent3d {
			width: SIDE as u32,
			height: SIDE as u32,
			depth_or_array_layers: 1,
		},
		TextureDimension::D2,
		data,
		TextureFormat::Rgba8UnormSrgb,
		RenderAssetUsages::default(),
	)
}

fn make_blur(
	mut commands: Commands,
	mut meshes: ResMut<Assets<Mesh>>,
	mut images: ResMut<Assets<Image>>,
	mut materials: ResMut<Assets<StandardMaterial>>,
) {
	commands.insert_resource(Blur {
		// The circle faces along Z, which is the axis of the nose of a plane.
		mesh: meshes.add(Circle::new(1.0)),
		material: materials.add(StandardMaterial {
			base_color_texture: Some(images.add(picture())),
			alpha_mode: AlphaMode::Blend,
			unlit: true,
			// A pilot sees the disc from behind, and an enemy sees it from the front.
			cull_mode: None,
			double_sided: true,
			..default()
		}),
	});
}

fn turn(time: Res<Time>, mut discs: Query<&mut Transform, With<Turning>>) {
	for mut disc in &mut discs {
		disc.rotate_local_z(TURN_RATE * time.delta_secs());
	}
}

//! The sky: a dome that fades from the haze of the horizon to the blue of the zenith, the sun and
//! its light, the distance fog, and the clouds.
//!
//! The clouds have fixed places and shapes from a fixed seed, so they are the same on every
//! machine of a match, and a pilot can hide in one. The hook `clouds=0` in the address leaves them out.

use std::f32::consts::TAU;

use bevy::{camera::Exposure, light::NotShadowCaster, mesh::VertexAttributeValues, prelude::*};

use crate::{
	page,
	puff::{self, Billboard, PuffAssets},
	view::Drawn,
};

/// The colour of the haze at the horizon. The distance fog has the same colour, so the far ground fades into the sky.
const HORIZON: Color = Color::srgb(0.75, 0.83, 0.91);
/// The colour of the sky straight up.
const ZENITH: Color = Color::srgb(0.22, 0.43, 0.78);
/// How high the haze reaches above the horizon: a larger number keeps the haze lower.
const HAZE_FALLOFF: f32 = 0.45;
/// The direction toward the sun, not yet of length 1. The sun stands 30 degrees above the horizon.
const SUN_TOWARD: Vec3 = Vec3::new(-0.62, 0.577, -0.78);
const SUN_COLOUR: Color = Color::srgb(1.0, 0.95, 0.86);
/// The light of the sun on a surface that faces it, in lux.
const SUN_ILLUMINANCE: f32 = 10_000.0;
/// The angular radius of the disc of the sun, in radians. It is larger than the real sun, so that a pilot notices it.
const SUN_RADIUS: f32 = 0.03;
/// The brightness of the disc of the sun, as a multiple of white.
const SUN_BRIGHTNESS: f32 = 6.0;
/// The colour of the glow of the haze around the sun, with its strength as the alpha.
const SUN_GLOW: Color = Color::srgba(1.0, 0.9, 0.72, 0.04);
/// How narrow the glow of the haze around the sun is: a larger number makes it narrower.
const SUN_GLOW_EXPONENT: f32 = 8.0;
/// The brightness of the glow just around the disc of the sun, as a multiple of white.
const CORONA: f32 = 1.2;
/// The angular radius of the glow around the disc of the sun, in radians.
const CORONA_RADIUS: f32 = 0.15;
/// The light that fills the shade, so that the side of a plane away from the sun stays readable.
const AMBIENT_COLOUR: Color = Color::srgb(0.78, 0.86, 1.0);
/// The brightness of the light in the shade, in candela per square metre.
const AMBIENT_BRIGHTNESS: f32 = 900.0;
/// The distances over which the fog thickens from nothing to full, in metres.
const FOG_START: f32 = 2000.0;
const FOG_END: f32 = 6500.0;
/// The radius of the dome of the sky around the camera, in metres. It stays inside the far plane of the camera.
const DOME_RADIUS: f32 = 8000.0;
/// The distance of the disc of the sun from the camera, in metres. It stands in front of the dome.
const SUN_DISTANCE: f32 = 7500.0;
/// The number of slices around and from the bottom to the top of the dome.
const DOME_DETAIL: (u32, u32) = (64, 48);

/// The seed from which every machine places the same clouds.
const CLOUD_SEED: u32 = 0x5eed_c10d;
/// The number of clouds over the arena.
const ARENA_CLOUDS: u32 = 22;
/// The number of clouds around the arena, which give the sky depth.
const FAR_CLOUDS: u32 = 14;
/// How far from the centre the clouds over the arena stand, in metres.
const ARENA_CLOUD_REACH: f32 = 2400.0;
/// The ring in which the clouds around the arena stand: from, to, in metres from the centre.
const FAR_CLOUD_RING: (f32, f32) = (3000.0, 6000.0);
/// The height of the bottom of a cloud: from, to, in metres.
const CLOUD_BASE: (f32, f32) = (700.0, 1100.0);
/// The width of a cloud: from, to, in metres.
const CLOUD_WIDTH: (f32, f32) = (240.0, 520.0);
/// The height of a cloud, as a part of its width.
const CLOUD_HEIGHT: f32 = 0.5;
/// The number of puffs in a cloud of 100 m width. A wider cloud has more.
const PUFFS_PER_100_M: f32 = 6.0;
/// The size of a puff, as a part of the width of its cloud: from, to.
const PUFF_SIZE: (f32, f32) = (0.35, 0.6);
/// The colour of the shaded bottom of a cloud, and of its sunlit top.
const CLOUD_BOTTOM: Color = Color::srgb(0.74, 0.78, 0.86);
const CLOUD_TOP: Color = Color::srgb(1.0, 0.99, 0.96);
/// How much of the sky behind it a puff of cloud covers, from 0 to 1.
const CLOUD_COVER: f32 = 0.9;
/// The number of shades of a cloud from its bottom to its top.
const SHADES: usize = 4;
/// The number of steps in which a puff fades when the camera comes near.
const FADES: usize = 5;
/// The distance from the camera, as a multiple of the size of a puff, at which a puff starts to fade.
const FADE_START: f32 = 1.5;
/// The distance from the camera, as a multiple of the size of a puff, at which a puff is gone.
const FADE_END: f32 = 0.6;

/// The dome of the sky and the disc of the sun, which move with the camera.
#[derive(Component)]
struct FollowsCamera(f32);

/// A puff of a cloud of the sky.
#[derive(Component)]
struct CloudPuff {
	/// The shade from the bottom to the top of its cloud.
	shade: usize,
	/// The side of its square, in metres.
	size: f32,
	/// The step of its fade near the camera, from 0 for gone to [`FADES`] for whole.
	fade: usize,
}

/// The materials of the puffs of the clouds, for each shade and each step of the fade.
#[derive(Resource)]
struct CloudLooks([[Handle<StandardMaterial>; FADES]; SHADES]);

/// Return the direction toward the sun, from which the light of the scene comes.
pub fn sun() -> Vec3 {
	SUN_TOWARD.normalize()
}

pub fn plugin(app: &mut App) {
	app.insert_resource(ClearColor(HORIZON))
		.insert_resource(GlobalAmbientLight {
			color: AMBIENT_COLOUR,
			brightness: AMBIENT_BRIGHTNESS,
			..default()
		})
		.add_systems(Startup, (make_sky, make_clouds))
		.add_systems(Update, (add_fog, (follow_camera, fade_clouds).after(Drawn)));
}

/// Return the strength of the light of the sun in the picture, as the fog of the engine computes it.
fn sun_in_picture() -> LinearRgba {
	SUN_COLOUR.to_linear() * (SUN_ILLUMINANCE * Exposure::default().exposure())
}

/// Return the colour of the sky in a direction, with the glow of the haze around the sun as the fog adds it.
fn sky_colour(direction: Vec3) -> LinearRgba {
	let up = (direction.y.max(0.0)).powf(HAZE_FALLOFF);
	let (horizon, zenith) = (HORIZON.to_linear(), ZENITH.to_linear());
	let toward = direction.dot(sun()).max(0.0);
	let (light, glow) = (sun_in_picture(), SUN_GLOW.to_linear());
	let strength = glow.alpha * toward.powf(SUN_GLOW_EXPONENT);
	let haze = LinearRgba::rgb(light.red * glow.red, light.green * glow.green, light.blue * glow.blue) * strength;
	let colour = horizon * (1.0 - up) + zenith * up + haze;
	colour.with_alpha(1.0)
}

fn make_sky(
	mut commands: Commands,
	assets: Res<PuffAssets>,
	mut meshes: ResMut<Assets<Mesh>>,
	mut materials: ResMut<Assets<StandardMaterial>>,
) {
	let mut dome = Sphere::new(1.0).mesh().uv(DOME_DETAIL.0, DOME_DETAIL.1);
	if let Some(VertexAttributeValues::Float32x3(positions)) = dome.attribute(Mesh::ATTRIBUTE_POSITION) {
		let colours: Vec<[f32; 4]> = positions
			.iter()
			.map(|&position| sky_colour(Vec3::from(position)).to_f32_array())
			.collect();
		dome.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
	}
	let backdrop = |colour: LinearRgba| StandardMaterial {
		base_color: colour.into(),
		unlit: true,
		fog_enabled: false,
		cull_mode: None,
		..default()
	};
	commands.spawn((
		FollowsCamera(0.0),
		Mesh3d(meshes.add(dome)),
		MeshMaterial3d(materials.add(backdrop(LinearRgba::WHITE))),
		Transform::from_scale(Vec3::splat(DOME_RADIUS)),
		NotShadowCaster,
	));
	commands.spawn((
		FollowsCamera(SUN_DISTANCE),
		Billboard,
		Mesh3d(meshes.add(Circle::new(SUN_DISTANCE * SUN_RADIUS.tan()))),
		MeshMaterial3d(materials.add(backdrop(SUN_COLOUR.to_linear() * SUN_BRIGHTNESS))),
		NotShadowCaster,
	));
	// The glow stands just behind the disc, so that the disc covers its middle.
	commands.spawn((
		FollowsCamera(SUN_DISTANCE + 1.0),
		Billboard,
		Mesh3d(assets.square.clone()),
		MeshMaterial3d(materials.add(StandardMaterial {
			base_color_texture: Some(assets.glow.clone()),
			alpha_mode: AlphaMode::Add,
			..backdrop(SUN_COLOUR.to_linear() * CORONA)
		})),
		Transform::from_scale(Vec3::splat(2.0 * SUN_DISTANCE * CORONA_RADIUS.tan())),
		NotShadowCaster,
	));
	commands.spawn((
		DirectionalLight {
			color: SUN_COLOUR,
			illuminance: SUN_ILLUMINANCE,
			shadow_maps_enabled: false,
			..default()
		},
		Transform::default().looking_to(-sun(), Vec3::Y),
	));
}

/// Give the camera the distance fog, in the colour of the haze at the horizon.
fn add_fog(mut commands: Commands, cameras: Query<Entity, Added<Camera3d>>) {
	for camera in &cameras {
		commands.entity(camera).insert(DistanceFog {
			color: HORIZON,
			directional_light_color: SUN_GLOW,
			directional_light_exponent: SUN_GLOW_EXPONENT,
			falloff: FogFalloff::Linear {
				start: FOG_START,
				end: FOG_END,
			},
		});
	}
}

/// Keep the dome around the camera, and the sun at its place in the sky.
fn follow_camera(
	camera: Single<&Transform, (With<Camera3d>, Without<FollowsCamera>)>,
	mut backdrops: Query<(&FollowsCamera, &mut Transform)>,
) {
	for (follows, mut transform) in &mut backdrops {
		transform.translation = camera.translation + sun() * follows.0;
	}
}

/// Place the clouds from the fixed seed. Each cloud is a heap of puffs with a flat bottom and a round top.
fn make_clouds(mut commands: Commands, assets: Res<PuffAssets>, mut materials: ResMut<Assets<StandardMaterial>>) {
	let looks = std::array::from_fn(|shade| {
		let height = shade as f32 / (SHADES - 1) as f32;
		let colour = CLOUD_BOTTOM.to_linear() * (1.0 - height) + CLOUD_TOP.to_linear() * height;
		std::array::from_fn(|fade| {
			let cover = CLOUD_COVER * (fade + 1) as f32 / FADES as f32;
			materials.add(StandardMaterial {
				base_color: colour.with_alpha(cover).into(),
				base_color_texture: Some(assets.cloud.clone()),
				unlit: true,
				alpha_mode: AlphaMode::Blend,
				cull_mode: None,
				..default()
			})
		})
	});
	if page::query("clouds").as_deref() != Some("0") {
		let mut seed = CLOUD_SEED;
		let mut next = move || {
			seed = seed.wrapping_add(1);
			puff::random(seed)
		};
		let between = |(low, high): (f32, f32), along: f32| low + (high - low) * along;
		for cloud in 0..ARENA_CLOUDS + FAR_CLOUDS {
			// The clouds over the arena cover its disc evenly, and the others stand in a ring around it.
			let reach = if cloud < ARENA_CLOUDS {
				ARENA_CLOUD_REACH * next().sqrt()
			} else {
				between(FAR_CLOUD_RING, next())
			};
			let angle = next() * TAU;
			let width = between(CLOUD_WIDTH, next());
			let base = Vec3::new(angle.cos() * reach, between(CLOUD_BASE, next()), angle.sin() * reach);
			let count = (width / 100.0 * PUFFS_PER_100_M).round() as u32;
			for _ in 0..count {
				let size = width * between(PUFF_SIZE, next());
				let (turn, out) = (next() * TAU, next().sqrt());
				let across = (width - size) / 2.0 * out;
				// A puff near the middle of the cloud can stand higher, so the top is round.
				let rise = width * CLOUD_HEIGHT * (1.0 - out * out) * next();
				let place = base + Vec3::new(turn.cos() * across, size * 0.3 + rise, turn.sin() * across);
				let height = (rise / (width * CLOUD_HEIGHT)).clamp(0.0, 1.0);
				let shade = ((height * SHADES as f32) as usize).min(SHADES - 1);
				commands.spawn((
					CloudPuff {
						shade,
						size,
						fade: FADES,
					},
					Billboard,
					Mesh3d(assets.square.clone()),
					MeshMaterial3d(looks[shade][FADES - 1].clone()),
					Transform::from_translation(place).with_scale(Vec3::splat(size)),
					NotShadowCaster,
				));
			}
		}
	}
	commands.insert_resource(CloudLooks(looks));
}

/// Fade each puff of cloud that the camera comes near, so that a plane flies into a cloud and out of it smoothly.
fn fade_clouds(
	camera: Single<&Transform, (With<Camera3d>, Without<CloudPuff>)>,
	looks: Res<CloudLooks>,
	mut puffs: Query<(
		&mut CloudPuff,
		&Transform,
		&mut MeshMaterial3d<StandardMaterial>,
		&mut Visibility,
	)>,
) {
	for (mut puff, transform, mut material, mut visibility) in &mut puffs {
		let near = transform.translation.distance(camera.translation) / puff.size;
		let along = ((near - FADE_END) / (FADE_START - FADE_END)).clamp(0.0, 1.0);
		let fade = (along * FADES as f32).ceil() as usize;
		if fade == puff.fade {
			continue;
		}
		puff.fade = fade;
		match fade.checked_sub(1) {
			Some(step) => {
				*visibility = Visibility::Inherited;
				material.0 = looks.0[puff.shade][step].clone();
			}
			None => *visibility = Visibility::Hidden,
		}
	}
}

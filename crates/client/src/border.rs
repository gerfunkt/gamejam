//! The wall at the edge of the arena, where a plane turns back by itself.
//!
//! The wall is a grid that the player sees only near it. Where the plane of the player comes close,
//! a patch of the grid fades in around the nearest place of the wall, and the rest stays clear.

use bevy::{
	asset::RenderAssetUsages,
	image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
	mesh::Indices,
	prelude::*,
	render::render_resource::{Extent3d, PrimitiveTopology, TextureDimension, TextureFormat},
};
use sim::{Session, tuning};

use crate::{flow::Game, view::Drawn};

/// The columns of the wall around the arena, and its rows from below the ground to above the ceiling.
const COLUMNS: usize = 360;
const ROWS: usize = 16;
/// The bottom and the top of the wall, in metres.
const BOTTOM: f32 = -50.0;
const TOP: f32 = tuning::CEILING + 100.0;
/// The distance from the plane at which the wall starts to show, in metres.
const REVEAL: f32 = 650.0;
/// How much of the view the wall hides at its place nearest to the plane, from 0 to 1.
const STRONGEST: f32 = 0.9;
/// The side of one square of the grid on the wall, in metres.
const CELL: f32 = 40.0;
/// The side of the picture of one square, in pixels.
const PICTURE: usize = 64;
/// How much of the view the inside of a square hides, from 0 to 1. The lines hide all of it.
const FILL: f32 = 0.14;
/// The colour of the wall: a warning red, which no other thing in the sky has.
const COLOUR: Color = Color::srgb(1.0, 0.38, 0.28);

/// The mesh of the wall, which takes the nearness of the plane in its colours each frame.
#[derive(Component)]
struct Wall(Handle<Mesh>);

pub fn plugin(app: &mut App) {
	app.add_systems(Startup, make_wall)
		.add_systems(Update, reveal.after(Drawn));
}

/// Return the picture of one square of the grid: a bright line on two of its sides, and a faint inside.
fn picture() -> Image {
	let line = |at: usize| {
		let from_edge = at.min(PICTURE - at) as f32;
		(1.0 - (from_edge - 1.0) / 2.0).clamp(0.0, 1.0)
	};
	let mut data = Vec::with_capacity(PICTURE * PICTURE * 4);
	for row in 0..PICTURE {
		for column in 0..PICTURE {
			let cover = FILL + (1.0 - FILL) * line(column).max(line(row));
			data.extend([255, 255, 255, (cover * 255.0) as u8]);
		}
	}
	let mut image = Image::new(
		Extent3d {
			width: PICTURE as u32,
			height: PICTURE as u32,
			depth_or_array_layers: 1,
		},
		TextureDimension::D2,
		data,
		TextureFormat::Rgba8UnormSrgb,
		RenderAssetUsages::default(),
	);
	// The squares repeat along the wall.
	image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
		address_mode_u: ImageAddressMode::Repeat,
		address_mode_v: ImageAddressMode::Repeat,
		..ImageSamplerDescriptor::linear()
	});
	image
}

fn make_wall(
	mut commands: Commands,
	mut meshes: ResMut<Assets<Mesh>>,
	mut images: ResMut<Assets<Image>>,
	mut materials: ResMut<Assets<StandardMaterial>>,
) {
	let radius = tuning::ARENA_RADIUS;
	let (mut positions, mut normals, mut squares) = (Vec::new(), Vec::new(), Vec::new());
	for row in 0..=ROWS {
		let height = BOTTOM + (TOP - BOTTOM) * row as f32 / ROWS as f32;
		for column in 0..=COLUMNS {
			let angle = column as f32 / COLUMNS as f32 * std::f32::consts::TAU;
			let (sine, cosine) = angle.sin_cos();
			positions.push([cosine * radius, height, sine * radius]);
			normals.push([-cosine, 0.0, -sine]);
			squares.push([angle * radius / CELL, height / CELL]);
		}
	}
	let mut indices = Vec::new();
	let across = COLUMNS as u32 + 1;
	for row in 0..ROWS as u32 {
		for column in 0..COLUMNS as u32 {
			let corner = row * across + column;
			indices.extend([
				corner,
				corner + across,
				corner + 1,
				corner + 1,
				corner + across,
				corner + across + 1,
			]);
		}
	}
	let clear = vec![[0.0_f32; 4]; positions.len()];
	let mesh = meshes.add(
		Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
			.with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
			.with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
			.with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, squares)
			.with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, clear)
			.with_inserted_indices(Indices::U32(indices)),
	);
	commands.spawn((
		Wall(mesh.clone()),
		Mesh3d(mesh),
		MeshMaterial3d(materials.add(StandardMaterial {
			base_color_texture: Some(images.add(picture())),
			alpha_mode: AlphaMode::Blend,
			unlit: true,
			// A pilot sees the wall from inside the arena, and from outside it after a turn too wide.
			cull_mode: None,
			double_sided: true,
			..default()
		})),
		Visibility::Hidden,
	));
}

/// Show the wall near the plane of the player, and hide it everywhere else.
fn reveal(game: Res<Game>, wall: Single<(&Wall, &mut Visibility)>, mut meshes: ResMut<Assets<Mesh>>) {
	let (wall, mut visibility) = wall.into_inner();
	let plane = game
		.session
		.as_ref()
		.and_then(Session::game)
		.and_then(|running| running.plane(running.me()))
		.map(|plane| plane.position);
	let near = plane.filter(|place| tuning::ARENA_RADIUS - Vec2::new(place.x, place.z).length() < REVEAL);
	let Some(place) = near else {
		if *visibility != Visibility::Hidden {
			*visibility = Visibility::Hidden;
		}
		return;
	};
	*visibility = Visibility::Inherited;
	let Some(mut mesh) = meshes.get_mut(&wall.0) else {
		return;
	};
	let Some(positions) = mesh
		.attribute(Mesh::ATTRIBUTE_POSITION)
		.and_then(|positions| positions.as_float3())
	else {
		return;
	};
	let colour = COLOUR.to_linear();
	let colours: Vec<[f32; 4]> = positions
		.iter()
		.map(|&corner| {
			let nearness = (1.0 - place.distance(Vec3::from(corner)) / REVEAL).max(0.0);
			colour.with_alpha(STRONGEST * nearness * nearness).to_f32_array()
		})
		.collect();
	mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
}

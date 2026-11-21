//! The land beyond the map, out to the horizon.
//!
//! The map covers a square. Rings of land carry the heights and the colours of its edge out to a plain
//! of low hills. The distance fog turns the far rings into the haze of the horizon, which has the
//! colour of the lower half of the sky, so the land ends nowhere that a pilot can see.

use bevy::{asset::RenderAssetUsages, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology};
use sim::{arena, tuning};

/// How far each ring of land lies beyond the edge of the map, in metres. The last rings lie in the haze.
const RINGS: [f32; 9] = [0.0, 250.0, 600.0, 1200.0, 2200.0, 3600.0, 5500.0, 8000.0, 12000.0];
/// How far beyond the edge of the map its heights give way to the hills of the plain, in metres.
const HEIGHT_BLEND: f32 = 1500.0;
/// How far beyond the edge of the map its colours give way to the colour of the plain, in metres.
const COLOUR_BLEND: f32 = 1200.0;
/// The height of the highest hills of the plain, in metres.
const HILLS: f32 = 90.0;
/// The colour of the plain until the picture of the map arrives.
const PLAIN: Color = Color::srgb(0.45, 0.42, 0.31);

/// The land beyond the map, which takes its colours from the picture of the map once it arrives.
#[derive(Resource)]
struct Beyond {
	picture: Handle<Image>,
	mesh: Handle<Mesh>,
	coloured: bool,
}

pub fn plugin(app: &mut App) {
	app.add_systems(Startup, make_land).add_systems(Update, colour_land);
}

/// Return the points of the height grid on the edge of the map, once around, as their columns and rows.
fn edge(side: usize) -> Vec<(usize, usize)> {
	let last = side - 1;
	let north = (0..last).map(|step| (step, 0));
	let east = (0..last).map(|step| (last, step));
	let south = (0..last).map(move |step| (last - step, last));
	let west = (0..last).map(move |step| (0, last - step));
	north.chain(east).chain(south).chain(west).collect()
}

/// Return the place of a point of the height grid, in metres.
fn place(column: usize, row: usize, side: usize) -> Vec2 {
	let at = |index: usize| (index as f32 / (side - 1) as f32 - 0.5) * tuning::TERRAIN_SIDE;
	Vec2::new(at(column), at(row))
}

/// Return how much of the edge of the map stays at `distance` beyond it: 1 at the edge, and 0 from `reach` on.
fn kept(distance: f32, reach: f32) -> f32 {
	let share = (distance / reach).clamp(0.0, 1.0);
	1.0 - share * share * (3.0 - 2.0 * share)
}

/// Return the height of the low hills of the plain at a place, in metres.
fn hills(at: Vec2) -> f32 {
	let rolling = (at.x * 0.0011 + 0.7).sin() * (at.y * 0.0009 - 0.3).cos();
	let long = ((at.x + at.y) * 0.000_45).sin();
	HILLS * (0.5 + 0.3 * rolling + 0.2 * long)
}

fn make_land(
	mut commands: Commands,
	assets: Res<AssetServer>,
	mut meshes: ResMut<Assets<Mesh>>,
	mut materials: ResMut<Assets<StandardMaterial>>,
) {
	let grid = arena::ground();
	let side = grid.side();
	let edge = edge(side);
	let half = tuning::TERRAIN_SIDE / 2.0;
	let mut positions = Vec::with_capacity(edge.len() * RINGS.len());
	for distance in RINGS {
		for &(column, row) in &edge {
			let at = place(column, row, side);
			let out = at * ((half + distance) / half);
			let share = kept(distance, HEIGHT_BLEND);
			let height = grid.point(column, row) * share + hills(out) * (1.0 - share);
			positions.push(Vec3::new(out.x, height, out.y));
		}
	}
	let around = edge.len() as u32;
	let mut indices = Vec::new();
	for ring in 0..RINGS.len() as u32 - 1 {
		for step in 0..around {
			let (inner, next) = (ring * around + step, ring * around + (step + 1) % around);
			let (outer, outer_next) = (inner + around, next + around);
			// Both triangles face up.
			indices.extend([inner, outer_next, outer, inner, next, outer_next]);
		}
	}
	let mut normals = vec![Vec3::ZERO; positions.len()];
	for triangle in indices.as_chunks::<3>().0 {
		let [a, b, c] = triangle.map(|corner| positions[corner as usize]);
		let face = (b - a).cross(c - a);
		for &corner in triangle {
			normals[corner as usize] += face;
		}
	}
	let colours = vec![PLAIN.to_linear().to_f32_array(); positions.len()];
	let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
		.with_inserted_attribute(
			Mesh::ATTRIBUTE_POSITION,
			positions.iter().map(|position| position.to_array()).collect::<Vec<_>>(),
		)
		.with_inserted_attribute(
			Mesh::ATTRIBUTE_NORMAL,
			normals
				.iter()
				.map(|normal| normal.normalize_or(Vec3::Y).to_array())
				.collect::<Vec<_>>(),
		)
		.with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colours)
		.with_inserted_indices(Indices::U32(indices));
	let mesh = meshes.add(mesh);
	commands.spawn((
		Mesh3d(mesh.clone()),
		MeshMaterial3d(materials.add(StandardMaterial {
			perceptual_roughness: 1.0,
			reflectance: 0.1,
			..default()
		})),
	));
	commands.insert_resource(Beyond {
		picture: assets.load("terrain/map.png"),
		mesh,
		coloured: false,
	});
}

/// Give the land the colours of the edge of the map, once the picture of the map arrived.
///
/// The corners of the picture lie beyond the model of the map, so their mean is the colour of the plain.
fn colour_land(mut beyond: ResMut<Beyond>, images: Res<Assets<Image>>, mut meshes: ResMut<Assets<Mesh>>) {
	if beyond.coloured {
		return;
	}
	let Some(picture) = images.get(&beyond.picture) else {
		return;
	};
	let (Some(data), width, height) = (
		picture.data.as_ref(),
		picture.width() as usize,
		picture.height() as usize,
	) else {
		return;
	};
	// The engine holds an 8-bit picture as four bytes for each pixel, in the colours of the screen.
	if data.len() != width * height * 4 {
		beyond.coloured = true;
		return;
	}
	let pixel = |x: usize, y: usize| {
		let at = (y * width + x) * 4;
		Color::srgb_u8(data[at], data[at + 1], data[at + 2]).to_linear()
	};
	let corners = [
		pixel(0, 0),
		pixel(width - 1, 0),
		pixel(0, height - 1),
		pixel(width - 1, height - 1),
	];
	let plain = corners
		.iter()
		.fold(LinearRgba::NONE, |sum, corner| sum + *corner * 0.25);
	let side = arena::ground().side();
	let edge = edge(side);
	let to_pixel = |index: usize, size: usize| (index * (size - 1) + (side - 1) / 2) / (side - 1);
	let colours: Vec<[f32; 4]> = RINGS
		.iter()
		.flat_map(|&distance| {
			let share = kept(distance, COLOUR_BLEND);
			edge.iter().map(move |&(column, row)| {
				let border = pixel(to_pixel(column, width), to_pixel(row, height));
				(border * share + plain * (1.0 - share)).with_alpha(1.0).to_f32_array()
			})
		})
		.collect();
	if let Some(mut mesh) = meshes.get_mut(&beyond.mesh) {
		mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
	}
	beyond.coloured = true;
}

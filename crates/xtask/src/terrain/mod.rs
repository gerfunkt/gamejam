//! The terrain: from the raw model of the artist to the two small files that the game uses.
//!
//! `cargo xtask terrain` reads `assets/terrain/terrain.toml`. It turns the model so that up is +Y, scales
//! it to the square of [`sim::tuning::TERRAIN_SIDE`] metres, lowers it to height 0, and squeezes
//! its heights to at most [`sim::tuning::TERRAIN_MAX_RISE`] metres. Then it looks at the model from above,
//! and writes two files:
//!
//! - `crates/sim/src/arena/heights.bin`: the height of the highest surface at each point of a square grid,
//!   for the rules ([`sim::arena::HeightGrid`] defines the format).
//! - `web/assets/terrain/map.png`: the colour of the highest surface at each place, for the client. Its
//!   edges are the edges of the square, so the client stretches it over the ground.
//!
//! Git keeps both files, because the raw model is not in git.

use std::{fs, path::Path, time::Instant};

use anyhow::{Context, Result, bail, ensure};
use glam::Vec3;
use sim::{arena::HeightGrid, tuning};

use self::raster::{Lattice, top_surface};
use crate::{
	image::Image,
	mesh::{
		fbx,
		geometry::{Triangle, bounds},
		orient::{Orientation, orient},
		scene,
	},
};

mod manifest;
mod preview;
mod raster;

/// The samples of the colour picture along each side of a pixel. Their mean is the colour of the pixel.
const SAMPLES: usize = 2;
/// How far beyond the edge of the model the land sinks to the lowest height of the model, in metres.
///
/// The model need not fill the square. The land beyond it slopes down to a plain, which the client
/// continues to the horizon.
const SINK_DISTANCE: f32 = 900.0;
/// How far beyond the edge of the model its colours fade to their mean colour, in metres.
const COLOUR_FADE: f32 = 300.0;
/// The passes of smoothing over the land beyond the model. Each pass blends a point with its four neighbours.
const SMOOTH_PASSES: usize = 40;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// The numbers of a bake.
#[derive(Clone, Copy, Debug)]
struct Settings {
	/// The side of the square that the terrain covers, in metres.
	side: f32,
	/// The number of points on a side of the height grid.
	points: usize,
	/// The number of pixels on a side of the colour picture.
	picture: usize,
	/// The most that the terrain may rise above its lowest point, in metres.
	max_rise: f32,
	/// How far beyond the model the land sinks, and how far its colours fade, in metres.
	sink_distance: f32,
	colour_fade: f32,
}

/// The result of a bake.
struct Baked {
	/// The heights for the rules.
	grid: HeightGrid,
	/// The colours for the client.
	picture: Image,
	/// The metres for each unit of the model.
	scale: f32,
	/// The factor by which the heights were squeezed, from 0 to 1. It is 1 if they were not.
	squeeze: f32,
}

/// Scale the triangles by one factor in all three directions, so that the longer side of the model fits
/// `side` metres, and centre them over the origin in X and Z. Return the factor.
///
/// A model that is not square leaves the rest of the square empty. The bake fills it from the nearest edge.
///
/// # Errors
///
/// Returns an error if the model has no extent in X or in Z.
fn fit_to_square(triangles: &mut [Triangle], side: f32) -> Result<f32> {
	let Some((low, high)) = bounds(triangles) else {
		bail!("the model has no triangles");
	};
	let size = high - low;
	ensure!(
		size.x > 0.0 && size.z > 0.0,
		"the model is flat; check `north` and `up` in terrain.toml"
	);
	let scale = side / size.x.max(size.z);
	let centre = (low + high) / 2.0;
	let offset = Vec3::new(-centre.x, 0.0, -centre.z) * scale;
	for vertex in triangles.iter_mut().flatten() {
		vertex.position = vertex.position * scale + offset;
	}
	Ok(scale)
}

/// Lower `heights` so that the lowest is 0, and squeeze them so that none is above `max_rise`. Return the factor.
///
/// The factor is 1 if the highest height is already within `max_rise`. A squeeze keeps the shape of the
/// terrain and lowers the slopes by the same factor.
fn lower_and_squeeze(heights: &mut [f32], max_rise: f32) -> f32 {
	let low = heights.iter().copied().fold(f32::INFINITY, f32::min);
	let rise = heights.iter().copied().fold(f32::NEG_INFINITY, f32::max) - low;
	let squeeze = if rise > max_rise { max_rise / rise } else { 1.0 };
	for height in heights {
		*height = (*height - low) * squeeze;
	}
	squeeze
}

/// Bake the triangles, which stand with up as +Y, into the height grid and the colour picture.
///
/// # Errors
///
/// Returns an error if the model cannot fit the square or covers no point of it.
fn bake(triangles: &mut [Triangle], texture: &Image, settings: Settings) -> Result<Baked> {
	let scale = fit_to_square(triangles, settings.side)?;
	let surface = top_surface(triangles, Lattice::corners(settings.side, settings.points))?;
	let spacing = settings.side / (settings.points - 1) as f32;
	let base = surface
		.iter()
		.filter(|hit| hit.gap == 0.0)
		.map(|hit| hit.height)
		.fold(f32::INFINITY, f32::min);
	let mut heights: Vec<f32> = surface
		.iter()
		.map(|hit| base + (hit.height - base) * kept(hit.gap * spacing, settings.sink_distance))
		.collect();
	let beyond: Vec<bool> = surface.iter().map(|hit| hit.gap > 0.0).collect();
	smooth(&mut heights, settings.points, &beyond);
	let squeeze = lower_and_squeeze(&mut heights, settings.max_rise);
	let grid = HeightGrid::from_heights(settings.points, &heights)?;

	let samples = settings.picture * SAMPLES;
	let sample_spacing = settings.side / samples as f32;
	let hits = top_surface(triangles, Lattice::centres(settings.side, samples))?;
	// The colour of each pixel, and the mean distance of its samples to the model, in metres.
	let (colours, gaps): (Vec<[f32; 3]>, Vec<f32>) = (0..settings.picture)
		.flat_map(|row| (0..settings.picture).map(move |column| (column, row)))
		.map(|(column, row)| {
			let (mut sum, mut gap) = ([0.0_f32; 3], 0.0);
			for (sample_row, sample_column) in (0..SAMPLES).flat_map(|r| (0..SAMPLES).map(move |c| (r, c))) {
				let hit = hits[(row * SAMPLES + sample_row) * samples + column * SAMPLES + sample_column];
				let colour = texture.sample(hit.uv);
				sum.iter_mut().zip(colour).for_each(|(sum, channel)| *sum += channel);
				gap += hit.gap * sample_spacing;
			}
			let count = (SAMPLES * SAMPLES) as f32;
			(sum.map(|total| total / count), gap / count)
		})
		.unzip();
	let on_model: Vec<&[f32; 3]> = colours
		.iter()
		.zip(&gaps)
		.filter(|(_, gap)| **gap == 0.0)
		.map(|(colour, _)| colour)
		.collect();
	let mean = on_model
		.iter()
		.fold([0.0_f32; 3], |sum, colour| {
			std::array::from_fn(|channel| sum[channel] + colour[channel])
		})
		.map(|total| total / on_model.len().max(1) as f32);
	let beyond: Vec<bool> = gaps.iter().map(|gap| *gap > 0.0).collect();
	let mut channels: [Vec<f32>; 3] = std::array::from_fn(|channel| {
		colours
			.iter()
			.zip(&gaps)
			.map(|(colour, gap)| mean[channel] + (colour[channel] - mean[channel]) * kept(*gap, settings.colour_fade))
			.collect()
	});
	for channel in &mut channels {
		smooth(channel, settings.picture, &beyond);
	}
	let pixels = (0..colours.len())
		.map(|slot| std::array::from_fn(|channel| channels[channel][slot].round().clamp(0.0, 255.0) as u8))
		.collect();
	Ok(Baked {
		grid,
		picture: Image {
			width: settings.picture,
			height: settings.picture,
			pixels,
		},
		scale,
		squeeze,
	})
}

/// Return how much of a feature of the model stays at `distance` beyond its edge: 1 at the edge, and 0 from `reach` on.
fn kept(distance: f32, reach: f32) -> f32 {
	let share = (distance / reach).clamp(0.0, 1.0);
	1.0 - share * share * (3.0 - 2.0 * share)
}

/// Blend each value that `free` marks with its four neighbours, [`SMOOTH_PASSES`] times. The other values stay.
///
/// The values are a square of `side` by `side`, row by row.
fn smooth(values: &mut [f32], side: usize, free: &[bool]) {
	for _ in 0..SMOOTH_PASSES {
		let before = values.to_vec();
		for (slot, value) in values.iter_mut().enumerate().filter(|(slot, _)| free[*slot]) {
			let (column, row) = (slot % side, slot / side);
			let at = |column: usize, row: usize| before[row * side + column];
			*value = (at(column.saturating_sub(1), row)
				+ at((column + 1).min(side - 1), row)
				+ at(column, row.saturating_sub(1))
				+ at(column, (row + 1).min(side - 1)))
				/ 4.0;
		}
	}
}

/// Bake the terrain of the manifest, and write the height grid and the colour picture.
///
/// With `preview`, it also writes `assets/terrain/map-preview.png`: the heights as grey shades beside
/// the colours, with the circle of the arena on both.
pub fn run(root: &Path, preview: bool) -> Result<()> {
	let started = Instant::now();
	let assets = root.join("assets/terrain");
	let terrain = manifest::read(&assets.join("terrain.toml"))?;
	let read = |path: &Path| {
		fs::read(assets.join(path)).with_context(|| {
			format!(
				"cannot read {}; extract the archive `{}` into assets/terrain/raw/ as terrain.toml says",
				assets.join(path).display(),
				terrain.archive
			)
		})
	};
	let texture = read(&terrain.texture)?;
	ensure!(texture.starts_with(PNG_SIGNATURE), "the texture is not a PNG file");
	let texture = Image::decode(&texture).context("cannot read the texture")?;
	let mut triangles = {
		let nodes = fbx::parse(&read(&terrain.model)?).context("cannot read the model")?;
		scene::read(&nodes).context("cannot read the meshes of the model")?
	};
	let count = triangles.len();
	let up = Orientation {
		nose: terrain.north,
		up: terrain.up,
		pitch: 0.0,
	};
	orient(&mut triangles, up)?;
	let baked = bake(
		&mut triangles,
		&texture,
		Settings {
			side: tuning::TERRAIN_SIDE,
			points: terrain.points,
			picture: terrain.picture,
			max_rise: tuning::TERRAIN_MAX_RISE,
			sink_distance: SINK_DISTANCE,
			colour_fade: COLOUR_FADE,
		},
	)?;

	let heights = baked.grid.to_bytes();
	let heights_path = root.join("crates/sim/src/arena/heights.bin");
	fs::write(&heights_path, &heights).with_context(|| format!("cannot write {}", heights_path.display()))?;
	let map = baked.picture.encode()?;
	let map_path = root.join("web/assets/terrain/map.png");
	fs::create_dir_all(map_path.parent().context("the map has no directory")?)?;
	fs::write(&map_path, &map).with_context(|| format!("cannot write {}", map_path.display()))?;
	if preview {
		let path = assets.join("map-preview.png");
		fs::write(&path, preview::render(&baked.grid, &baked.picture).encode()?)
			.with_context(|| format!("cannot write {}", path.display()))?;
	}
	println!(
		"{}: {count} triangles, {:.3} m for each unit, heights {:.1} to {:.1} m{}, grid {points}x{points} points {} kB, picture {} kB, {:.1} s",
		terrain.key,
		baked.scale,
		baked.grid.low(),
		baked.grid.high(),
		if baked.squeeze < 1.0 {
			format!(" (squeezed by {:.3})", baked.squeeze)
		} else {
			String::new()
		},
		heights.len() / 1000,
		map.len() / 1000,
		started.elapsed().as_secs_f64(),
		points = terrain.points
	);
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::mesh::geometry::tests::vertex;

	fn triangle(corners: [[f32; 3]; 3]) -> Triangle {
		corners.map(|[x, y, z]| vertex(x, y, z))
	}

	const SETTINGS: Settings = Settings {
		side: 200.0,
		points: 5,
		picture: 4,
		max_rise: 500.0,
		sink_distance: 50.0,
		colour_fade: 20.0,
	};

	/// A strip of 2 by 20 units along Z that rises from 0 at its left edge to 10 units at its right edge.
	/// Its texture is red on the left half and green on the right half.
	fn ramp() -> Vec<Triangle> {
		let corner = |x: f32, z: f32| {
			let mut corner = vertex(x, 5.0 + 5.0 * x, z);
			corner.uv = glam::Vec2::new((x + 1.0) / 2.0, 0.5);
			corner
		};
		vec![
			[corner(-1.0, -10.0), corner(1.0, -10.0), corner(1.0, 10.0)],
			[corner(-1.0, -10.0), corner(1.0, 10.0), corner(-1.0, 10.0)],
		]
	}

	/// A pyramid with a base of 20 by 20 units and a height of 10 units.
	fn pyramid() -> Vec<Triangle> {
		let apex = [0.0, 10.0, 0.0];
		let corners = [
			[-10.0, 0.0, -10.0],
			[10.0, 0.0, -10.0],
			[10.0, 0.0, 10.0],
			[-10.0, 0.0, 10.0],
		];
		(0..4)
			.map(|side| triangle([corners[side], corners[(side + 1) % 4], apex]))
			.collect()
	}

	fn grey() -> Image {
		Image::filled(2, 2, [90, 90, 90])
	}

	#[test]
	fn a_long_model_fits_its_longer_side_to_the_square_and_scales_the_height_with_it() {
		let mut triangles = [triangle([[0.0, 0.0, 0.0], [8.0, 2.0, 0.0], [0.0, 0.0, 10.0]])];
		let scale = fit_to_square(&mut triangles, 100.0).unwrap();
		let (low, high) = bounds(&triangles).unwrap();
		assert_eq!(scale, 10.0, "the longer side of 10 units gets 100 m");
		assert_eq!((low.z, high.z), (-50.0, 50.0), "centred in Z");
		assert_eq!((low.x, high.x), (-40.0, 40.0), "centred in X, and 80 m wide");
		assert_eq!(high.y - low.y, 20.0, "the height has the same scale");
	}

	#[test]
	fn a_flat_model_is_refused_with_a_hint() {
		let message = fit_to_square(
			&mut [triangle([[0.0, 0.0, 0.0], [0.0, 5.0, 0.0], [0.0, 0.0, 5.0]])],
			100.0,
		)
		.unwrap_err()
		.to_string();
		assert!(
			message.contains("terrain.toml"),
			"the message names the file to check: {message}"
		);
	}

	#[test]
	fn the_lowest_height_goes_to_zero() {
		let mut heights = [30.0, 50.0, 40.0];
		assert_eq!(
			lower_and_squeeze(&mut heights, 500.0),
			1.0,
			"a low rise is not squeezed"
		);
		assert_eq!(heights, [0.0, 20.0, 10.0], "lowered by 30 m");
	}

	#[test]
	fn a_rise_over_the_limit_is_squeezed_to_the_limit_and_keeps_its_shape() {
		let mut heights = [100.0, 500.0, 300.0, 900.0];
		let squeeze = lower_and_squeeze(&mut heights, 400.0);
		assert_eq!(squeeze, 0.5, "800 m of rise become 400 m");
		assert_eq!(
			heights,
			[0.0, 200.0, 100.0, 400.0],
			"every height is halved above the lowest"
		);
	}

	#[test]
	fn a_rise_equal_to_the_limit_is_not_squeezed() {
		let mut heights = [0.0, 500.0];
		assert_eq!(lower_and_squeeze(&mut heights, 500.0), 1.0, "exactly at the limit");
	}

	#[test]
	fn a_pyramid_bakes_to_a_peak_at_the_middle_point_of_the_grid() {
		let baked = bake(&mut pyramid(), &grey(), SETTINGS).unwrap();
		assert_eq!(baked.scale, 10.0, "20 units become 200 m");
		assert_eq!(baked.squeeze, 1.0, "100 m of rise are within the limit");
		let grid = &baked.grid;
		assert_eq!(
			(grid.low(), grid.high()),
			(0.0, 100.0),
			"the pyramid rises 100 m from the ground"
		);
		assert_eq!(grid.point(2, 2), 100.0, "the peak is at the middle point");
		assert!(
			(grid.point(3, 2) - 50.0).abs() < 0.01,
			"halfway down in X: {}",
			grid.point(3, 2)
		);
		assert!(
			(grid.point(2, 1) - 50.0).abs() < 0.01,
			"halfway down in Z: {}",
			grid.point(2, 1)
		);
		assert_eq!(grid.point(0, 0), 0.0, "the corner is at the foot");
	}

	#[test]
	fn a_pyramid_over_the_limit_is_squeezed_to_the_limit() {
		let baked = bake(
			&mut pyramid(),
			&grey(),
			Settings {
				max_rise: 25.0,
				..SETTINGS
			},
		)
		.unwrap();
		assert_eq!(baked.squeeze, 0.25, "100 m become 25 m");
		assert_eq!(baked.grid.high(), 25.0, "the peak reaches the limit");
	}

	#[test]
	fn the_picture_covers_the_square_edge_to_edge_with_the_colours_of_the_top_surface() {
		// A flat square of 20 units, with a texture that is red on its left half and green on its right half.
		let mut ground = [
			triangle([[-10.0, 0.0, -10.0], [10.0, 0.0, -10.0], [10.0, 0.0, 10.0]]),
			triangle([[-10.0, 0.0, -10.0], [10.0, 0.0, 10.0], [-10.0, 0.0, 10.0]]),
		];
		for triangle in &mut ground {
			for vertex in triangle {
				vertex.uv.x = (vertex.position.x + 10.0) / 20.0;
				vertex.uv.y = 0.5;
			}
		}
		let texture = Image {
			width: 2,
			height: 1,
			pixels: vec![[255, 0, 0], [0, 255, 0]],
		};
		let baked = bake(&mut ground, &texture, SETTINGS).unwrap();
		let picture = &baked.picture;
		assert_eq!((picture.width, picture.height), (4, 4), "the size of the picture");
		assert_eq!(picture.pixels[0], [255, 0, 0], "the first pixel of the top row is red");
		assert_eq!(picture.pixels[3], [0, 255, 0], "the last pixel of the top row is green");
		assert_eq!(
			picture.pixels[15],
			[0, 255, 0],
			"the last pixel of the last row is green"
		);
	}

	#[test]
	fn the_land_beyond_the_model_slopes_down_toward_the_lowest_height_of_the_model() {
		let settings = Settings {
			points: 21,
			picture: 20,
			..SETTINGS
		};
		let baked = bake(&mut ramp(), &grey(), settings).unwrap();
		// The strip is 20 m wide, so the middle row covers columns 9 to 11 and leaves 9 columns on each side.
		let row: Vec<f32> = (11..21).map(|column| baked.grid.point(column, 10)).collect();
		assert!(row[0] > 90.0, "the high edge of the strip stays: {}", row[0]);
		assert!(row[1] > 50.0, "the land next to it is still high: {}", row[1]);
		assert!(
			row.windows(2).all(|pair| pair[1] <= pair[0] + 1e-3),
			"the land falls without a step back up: {row:?}"
		);
		// The smoothing carries some of the height of the strip to the edge, and the client continues the slope.
		assert!(
			row[9] < 20.0,
			"the edge of the square is far below the strip: {}",
			row[9]
		);
	}

	#[test]
	fn the_land_far_beyond_the_model_has_the_mean_colour_of_the_model() {
		let texture = Image {
			width: 2,
			height: 1,
			pixels: vec![[255, 0, 0], [0, 255, 0]],
		};
		let settings = Settings {
			picture: 20,
			..SETTINGS
		};
		let baked = bake(&mut ramp(), &texture, settings).unwrap();
		// The smoothing pulls a little of the nearest edge of the model, which is green here, into the far land.
		let edge = baked.picture.pixels[10 * 20 + 19];
		assert!(
			edge[0].abs_diff(128) <= 16 && edge[1].abs_diff(128) <= 16 && edge[2] == 0,
			"about half red and half green: {edge:?}"
		);
	}

	#[test]
	fn a_higher_surface_hides_the_colours_below_it() {
		let square = |height: f32, u: f32| {
			let mut pair = [
				triangle([[-10.0, height, -10.0], [10.0, height, -10.0], [10.0, height, 10.0]]),
				triangle([[-10.0, height, -10.0], [10.0, height, 10.0], [-10.0, height, 10.0]]),
			];
			pair.iter_mut()
				.flatten()
				.for_each(|vertex| vertex.uv = glam::Vec2::new(u, 0.5));
			pair
		};
		let texture = Image {
			width: 2,
			height: 1,
			pixels: vec![[255, 0, 0], [0, 0, 255]],
		};
		let mut roof_and_floor: Vec<Triangle> = [square(0.0, 0.0), square(3.0, 1.0)].concat();
		let baked = bake(&mut roof_and_floor, &texture, SETTINGS).unwrap();
		assert!(
			baked.picture.pixels.iter().all(|pixel| *pixel == [0, 0, 255]),
			"only the roof shows"
		);
	}
}

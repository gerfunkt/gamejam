//! Draw the baked terrain into one picture, so that an agent without a viewer can check it.
//!
//! The left panel shows the height grid as grey shades from the lowest height in black to the highest in
//! white. The right panel shows the colour picture. Both panels show the same square with +X to the right
//! and +Z down. A red ring marks the arena on both.

use glam::Vec2;
use sim::{arena::HeightGrid, tuning};

use crate::image::Image;

const GAP: usize = 8;
const BACKGROUND: [u8; 3] = [28, 30, 36];
const RING: [u8; 3] = [255, 40, 40];

/// Return the height grid and the colour picture side by side, each with the ring of the arena.
#[must_use]
pub fn render(grid: &HeightGrid, picture: &Image) -> Image {
	let size = picture.width;
	let mut heights = Vec::with_capacity(size * size);
	for row in 0..size {
		for column in 0..size {
			let place = (Vec2::new(column as f32, row as f32) + 0.5) / size as f32 - 0.5;
			let world = place * tuning::TERRAIN_SIDE;
			let part = (grid.height(world.x, world.y) - grid.low()) / (grid.high() - grid.low()).max(f32::EPSILON);
			heights.push([(part.clamp(0.0, 1.0) * 255.0).round() as u8; 3]);
		}
	}
	let ring = tuning::ARENA_RADIUS / tuning::TERRAIN_SIDE * size as f32;
	let mark = |pixels: &mut [[u8; 3]]| {
		for (slot, pixel) in pixels.iter_mut().enumerate() {
			let offset =
				Vec2::new((slot % size) as f32 + 0.5, (slot / size) as f32 + 0.5) - Vec2::splat(size as f32 / 2.0);
			if (offset.length() - ring).abs() < 1.0 {
				*pixel = RING;
			}
		}
	};
	let mut colours = picture.pixels.clone();
	mark(&mut heights);
	mark(&mut colours);
	let pixels = (0..size)
		.flat_map(|row| {
			let span = row * size..(row + 1) * size;
			[&heights[span.clone()], &[BACKGROUND; GAP][..], &colours[span]].concat()
		})
		.collect();
	Image {
		width: size * 2 + GAP,
		height: size,
		pixels,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A grid that rises from 0 m at the west edge to 100 m at the east edge.
	fn slope() -> HeightGrid {
		HeightGrid::from_heights(2, &[0.0, 100.0, 0.0, 100.0]).unwrap()
	}

	#[test]
	fn the_heights_are_grey_shades_from_black_to_white_beside_the_colours() {
		let picture = Image::filled(100, 100, [10, 200, 30]);
		let preview = render(&slope(), &picture);
		assert_eq!((preview.width, preview.height), (208, 100), "two panels and a gap");
		let row = 5 * preview.width;
		assert!(
			preview.pixels[row][0] < 5,
			"the west edge is black: {:?}",
			preview.pixels[row]
		);
		assert!(
			preview.pixels[row + 99][0] > 250,
			"the east edge is white: {:?}",
			preview.pixels[row + 99]
		);
		assert_eq!(preview.pixels[row + 100], BACKGROUND, "the gap");
		assert_eq!(
			preview.pixels[row + 108],
			[10, 200, 30],
			"the colour panel starts after the gap"
		);
	}

	#[test]
	fn the_ring_of_the_arena_is_drawn_on_both_panels_at_its_radius() {
		let picture = Image::filled(640, 640, [10, 200, 30]);
		let preview = render(&slope(), &picture);
		// 2500 m of 6400 m on 640 pixels are 250 pixels from the centre.
		let middle = 320 * preview.width;
		assert_eq!(
			preview.pixels[middle + 320 + 250],
			RING,
			"east of the centre on the height panel"
		);
		assert_eq!(
			preview.pixels[middle + 648 + 320 - 250],
			RING,
			"west of the centre on the colour panel"
		);
		assert_ne!(preview.pixels[middle + 320], RING, "the centre is not on the ring");
	}
}

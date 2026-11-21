//! Draw the parts of a model into a PNG image, so that an agent without a viewer can check it.
//!
//! The image shows the plane from above, with +X to the right and the nose up, and from its
//! right side, with the nose to the right and up at the top. Each part has its own colour. Dark
//! faces are steep to the view. The outlines are the boxes of the rules, in the colour of their part.
//! The rasteriser has a depth buffer and runs on the CPU.

use anyhow::Result;
use glam::{Vec2, Vec3};
use sim::Part;

use crate::mesh::geometry::Triangle;

/// The pixels for each metre.
const SCALE: f32 = 60.0;
const WIDTH: usize = 800;
const TOP_HEIGHT: usize = 540;
const SIDE_HEIGHT: usize = 300;
const GAP: usize = 8;
const BACKGROUND: [u8; 3] = [28, 30, 36];

fn colour(part: Part) -> [u8; 3] {
	match part {
		Part::Fuselage => [190, 190, 190],
		Part::LeftWing => [235, 60, 60],
		Part::RightWing => [60, 200, 80],
		Part::Tail => [70, 120, 245],
		Part::Engine => [245, 205, 40],
	}
}

/// A view of the plane along one axis.
#[derive(Clone, Copy)]
enum View {
	/// From above: X to the right, the nose up.
	Top,
	/// From the right side of the plane: the nose to the right, up at the top.
	Side,
}

impl View {
	fn height(self) -> usize {
		match self {
			View::Top => TOP_HEIGHT,
			View::Side => SIDE_HEIGHT,
		}
	}

	/// The direction from the model toward the viewer.
	fn toward_viewer(self) -> Vec3 {
		match self {
			View::Top => Vec3::Y,
			View::Side => Vec3::X,
		}
	}

	/// Return the position in the panel, in pixels from its top left corner, and the distance to the viewer.
	fn project(self, point: Vec3) -> (Vec2, f32) {
		let centre = Vec2::new(WIDTH as f32 / 2.0, self.height() as f32 / 2.0);
		let (offset, depth) = match self {
			View::Top => (Vec2::new(point.x, point.z), -point.y),
			View::Side => (Vec2::new(-point.z, -point.y), -point.x),
		};
		(centre + offset * SCALE, depth)
	}
}

/// A panel of pixels with a depth buffer.
struct Canvas {
	height: usize,
	pixels: Vec<[u8; 3]>,
	depth: Vec<f32>,
}

impl Canvas {
	fn new(height: usize) -> Self {
		Self {
			height,
			pixels: vec![BACKGROUND; WIDTH * height],
			depth: vec![f32::INFINITY; WIDTH * height],
		}
	}

	/// Fill a triangle, and keep the pixels that are nearest to the viewer.
	fn fill(&mut self, corners: [(Vec2, f32); 3], paint: [u8; 3]) {
		let [(a, da), (b, db), (c, dc)] = corners;
		let area = (b - a).perp_dot(c - a);
		if area.abs() < 1e-6 {
			return;
		}
		let low = a.min(b).min(c).floor().max(Vec2::ZERO);
		let high = (a.max(b).max(c).ceil() + Vec2::ONE).min(Vec2::new(WIDTH as f32, self.height as f32));
		for y in low.y as usize..high.y as usize {
			for x in low.x as usize..high.x as usize {
				let point = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
				let weights = [
					(b - point).perp_dot(c - point),
					(c - point).perp_dot(a - point),
					(a - point).perp_dot(b - point),
				]
				.map(|weight| weight / area);
				if weights.iter().any(|weight| *weight < 0.0) {
					continue;
				}
				let depth = weights[0] * da + weights[1] * db + weights[2] * dc;
				let slot = y * WIDTH + x;
				if depth < self.depth[slot] {
					self.depth[slot] = depth;
					self.pixels[slot] = paint;
				}
			}
		}
	}

	/// Draw the outline of a rectangle, one pixel wide, over everything.
	fn outline(&mut self, from: Vec2, to: Vec2, paint: [u8; 3]) {
		let (low, high) = (from.min(to).round().as_ivec2(), from.max(to).round().as_ivec2());
		for x in low.x..=high.x {
			self.set(x, low.y, paint);
			self.set(x, high.y, paint);
		}
		for y in low.y..=high.y {
			self.set(low.x, y, paint);
			self.set(high.x, y, paint);
		}
	}

	fn set(&mut self, x: i32, y: i32, paint: [u8; 3]) {
		if let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y))
			&& x < WIDTH
			&& y < self.height
		{
			self.pixels[y * WIDTH + x] = paint;
		}
	}
}

fn draw(parts: &[(Part, Vec<Triangle>)], view: View) -> Canvas {
	let mut canvas = Canvas::new(view.height());
	for (part, triangles) in parts {
		for triangle in triangles {
			let [a, b, c] = triangle.map(|vertex| vertex.position);
			let facing = (b - a).cross(c - a).normalize_or_zero().dot(view.toward_viewer()).abs();
			let shade = 0.45 + 0.35 * facing;
			let paint = colour(*part).map(|channel| (f32::from(channel) * shade) as u8);
			canvas.fill(triangle.map(|vertex| view.project(vertex.position)), paint);
		}
	}
	for part in Part::ALL {
		let (low, high) = (part.centre() - part.half_extents(), part.centre() + part.half_extents());
		canvas.outline(view.project(low).0, view.project(high).0, colour(part));
	}
	canvas
}

/// Draw the parts from above and from the side, and return the PNG file.
///
/// # Errors
///
/// Returns an error if the PNG encoder fails.
pub fn render(parts: &[(Part, Vec<Triangle>)]) -> Result<Vec<u8>> {
	let height = TOP_HEIGHT + GAP + SIDE_HEIGHT;
	let mut pixels = Vec::with_capacity(WIDTH * height * 3);
	let mut push = |canvas: &Canvas| pixels.extend(canvas.pixels.iter().flatten());
	push(&draw(parts, View::Top));
	push(&Canvas::new(GAP));
	push(&draw(parts, View::Side));
	let mut file = Vec::new();
	let mut encoder = png::Encoder::new(&mut file, u32::try_from(WIDTH)?, u32::try_from(height)?);
	encoder.set_color(png::ColorType::Rgb);
	encoder.set_depth(png::BitDepth::Eight);
	encoder.write_header()?.write_image_data(&pixels)?;
	Ok(file)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::mesh::geometry::tests::vertex;

	#[test]
	fn a_filled_triangle_covers_its_area_in_pixels() {
		let mut canvas = Canvas::new(100);
		let corner = |x, y| (Vec2::new(x, y), 0.0);
		canvas.fill([corner(10.0, 10.0), corner(50.0, 10.0), corner(10.0, 30.0)], [1, 2, 3]);
		let covered = canvas.pixels.iter().filter(|pixel| **pixel == [1, 2, 3]).count();
		assert!(
			(covered as f32 - 400.0).abs() < 40.0,
			"about 40 * 20 / 2 pixels: {covered}"
		);
	}

	#[test]
	fn the_nearer_triangle_wins_where_two_overlap() {
		let mut canvas = Canvas::new(100);
		let triangle = |depth| {
			let corner = |x, y| (Vec2::new(x, y), depth);
			[corner(0.0, 0.0), corner(40.0, 0.0), corner(0.0, 40.0)]
		};
		canvas.fill(triangle(1.0), [9, 9, 9]);
		canvas.fill(triangle(0.0), [5, 5, 5]);
		canvas.fill(triangle(2.0), [7, 7, 7]);
		assert_eq!(
			canvas.pixels[5 * WIDTH + 5],
			[5, 5, 5],
			"the triangle with the smallest depth stays"
		);
	}

	#[test]
	fn the_nose_is_at_the_top_of_the_top_view_and_at_the_right_of_the_side_view() {
		let nose = Vec3::new(0.0, 0.0, -4.0);
		let (top, _) = View::Top.project(nose);
		let (side, _) = View::Side.project(nose);
		assert!(
			top.y < TOP_HEIGHT as f32 / 2.0,
			"the nose is above the centre in the top view"
		);
		assert!(
			side.x > WIDTH as f32 / 2.0,
			"the nose is right of the centre in the side view"
		);
		assert!(
			View::Top.project(Vec3::X).0.x > WIDTH as f32 / 2.0,
			"+X is to the right in the top view"
		);
		assert!(
			View::Side.project(Vec3::Y).0.y < SIDE_HEIGHT as f32 / 2.0,
			"up is above the centre in the side view"
		);
	}

	#[test]
	fn the_image_is_a_png_with_both_views() {
		let parts = [(
			Part::Tail,
			vec![[vertex(0.0, 0.0, 3.0), vertex(1.0, 0.0, 3.0), vertex(0.0, 1.0, 4.0)]],
		)];
		let file = render(&parts).unwrap();
		let decoder = png::Decoder::new(std::io::Cursor::new(file));
		let reader = decoder.read_info().unwrap();
		let info = reader.info();
		assert_eq!(
			(info.width as usize, info.height as usize),
			(WIDTH, TOP_HEIGHT + GAP + SIDE_HEIGHT),
			"two panels"
		);
	}
}

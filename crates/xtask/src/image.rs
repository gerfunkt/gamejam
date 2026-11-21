//! PNG files as plain 8-bit RGB pixels.

use std::io::Cursor;

use anyhow::{Context, Result, bail, ensure};
use glam::Vec2;

/// A picture of 8-bit RGB pixels, row by row from the top left.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
	/// The number of pixels in a row.
	pub width: usize,
	/// The number of rows.
	pub height: usize,
	/// The pixels, `width` times `height` of them.
	pub pixels: Vec<[u8; 3]>,
}

impl Image {
	/// Make a picture of one colour, for the tests.
	#[cfg(test)]
	#[must_use]
	pub fn filled(width: usize, height: usize, colour: [u8; 3]) -> Self {
		Self {
			width,
			height,
			pixels: vec![colour; width * height],
		}
	}

	/// Read a PNG file of 8-bit RGB or RGBA pixels. The alpha channel is dropped.
	///
	/// # Errors
	///
	/// Returns an error if the bytes are not a PNG file, or if the pixels are not 8-bit RGB or RGBA.
	pub fn decode(bytes: &[u8]) -> Result<Self> {
		let mut reader = png::Decoder::new(Cursor::new(bytes))
			.read_info()
			.context("the file is not a PNG file")?;
		let mut data = vec![0; reader.output_buffer_size().context("the PNG picture is too large")?];
		let info = reader
			.next_frame(&mut data)
			.context("the PNG picture does not decode")?;
		ensure!(
			info.bit_depth == png::BitDepth::Eight,
			"the PNG picture has {:?} bits for each channel, and only 8 are supported",
			info.bit_depth
		);
		let channels = match info.color_type {
			png::ColorType::Rgb => 3,
			png::ColorType::Rgba => 4,
			other => bail!("the PNG picture has the colour type {other:?}, and only RGB and RGBA are supported"),
		};
		let pixels = data[..info.buffer_size()]
			.chunks_exact(channels)
			.map(|pixel| [pixel[0], pixel[1], pixel[2]])
			.collect();
		Ok(Self {
			width: usize::try_from(info.width)?,
			height: usize::try_from(info.height)?,
			pixels,
		})
	}

	/// Return the PNG file of the picture.
	///
	/// # Errors
	///
	/// Returns an error if the PNG encoder fails.
	pub fn encode(&self) -> Result<Vec<u8>> {
		let mut file = Vec::new();
		let mut encoder = png::Encoder::new(&mut file, u32::try_from(self.width)?, u32::try_from(self.height)?);
		encoder.set_color(png::ColorType::Rgb);
		encoder.set_depth(png::BitDepth::Eight);
		encoder.set_compression(png::Compression::High);
		encoder.write_header()?.write_image_data(self.pixels.as_flattened())?;
		Ok(file)
	}

	/// Return the picture with at most `side` pixels in a row and in a column.
	///
	/// Each new pixel is the mean of a square block of old pixels. The block has a whole number of
	/// pixels on a side, so the picture can become smaller than `side`. A picture that fits stays as it is.
	#[must_use]
	pub fn shrunk(&self, side: usize) -> Self {
		let block = self.width.max(self.height).div_ceil(side.max(1));
		if block <= 1 {
			return self.clone();
		}
		let (width, height) = ((self.width / block).max(1), (self.height / block).max(1));
		let pixels = (0..height)
			.flat_map(|row| (0..width).map(move |column| (column, row)))
			.map(|(column, row)| {
				let mut sum = [0_usize; 3];
				let mut count = 0;
				for y in (row * block..(row + 1) * block).take_while(|y| *y < self.height) {
					for x in (column * block..(column + 1) * block).take_while(|x| *x < self.width) {
						for (total, channel) in sum.iter_mut().zip(self.pixels[y * self.width + x]) {
							*total += usize::from(channel);
						}
						count += 1;
					}
				}
				sum.map(|total| ((total + count / 2) / count) as u8)
			})
			.collect();
		Self { width, height, pixels }
	}

	/// Return the colour at the texture coordinates `uv`, blended between the four nearest pixels.
	///
	/// The origin of `uv` is at the bottom left, as in FBX. A place outside the picture has the colour of the nearest edge.
	#[must_use]
	pub fn sample(&self, uv: Vec2) -> [f32; 3] {
		let x = (uv.x * self.width as f32 - 0.5).clamp(0.0, (self.width - 1) as f32);
		let y = ((1.0 - uv.y) * self.height as f32 - 0.5).clamp(0.0, (self.height - 1) as f32);
		let (column, row) = (x as usize, y as usize);
		let (across, down) = (x - column as f32, y - row as f32);
		let (next_column, next_row) = ((column + 1).min(self.width - 1), (row + 1).min(self.height - 1));
		let pixel = |column: usize, row: usize| self.pixels[row * self.width + column].map(f32::from);
		let (a, b, c, d) = (
			pixel(column, row),
			pixel(next_column, row),
			pixel(column, next_row),
			pixel(next_column, next_row),
		);
		std::array::from_fn(|channel| {
			let top = a[channel] * (1.0 - across) + b[channel] * across;
			let bottom = c[channel] * (1.0 - across) + d[channel] * across;
			top * (1.0 - down) + bottom * down
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_shrunk_picture_has_the_mean_of_each_block_and_a_small_picture_stays() {
		let mut picture = Image::filled(4, 4, [10, 20, 30]);
		picture.pixels[0] = [50, 20, 30];
		let small = picture.shrunk(2);
		assert_eq!((small.width, small.height), (2, 2), "four pixels become one");
		assert_eq!(small.pixels[0], [20, 20, 30], "the mean of 50, 10, 10, and 10 is 20");
		assert_eq!(small.pixels[3], [10, 20, 30], "a block of one colour keeps it");
		assert_eq!(picture.shrunk(4), picture, "a picture that fits stays as it is");
		assert_eq!(
			Image::filled(5, 3, [1, 2, 3]).shrunk(2).width,
			1,
			"a block of three pixels fits once into five"
		);
	}

	fn two_by_one() -> Image {
		Image {
			width: 2,
			height: 1,
			pixels: vec![[255, 0, 0], [0, 255, 0]],
		}
	}

	#[test]
	fn a_picture_written_as_png_reads_back_the_same() {
		let image = Image {
			width: 3,
			height: 2,
			pixels: (0..6).map(|index| [index * 40, 255 - index, 7]).collect(),
		};
		assert_eq!(Image::decode(&image.encode().unwrap()).unwrap(), image, "a round trip");
	}

	#[test]
	fn a_file_that_is_no_png_picture_is_an_error() {
		assert!(Image::decode(b"not a picture").is_err(), "text is no picture");
	}

	#[test]
	fn the_colour_at_a_pixel_centre_is_the_colour_of_the_pixel_and_between_two_it_is_their_mean() {
		let image = two_by_one();
		assert_eq!(
			image.sample(Vec2::new(0.25, 0.5)),
			[255.0, 0.0, 0.0],
			"the centre of the left pixel"
		);
		assert_eq!(
			image.sample(Vec2::new(0.5, 0.5)),
			[127.5, 127.5, 0.0],
			"the border between the pixels"
		);
		assert_eq!(
			image.sample(Vec2::new(0.0, 0.5)),
			[255.0, 0.0, 0.0],
			"past the left edge"
		);
		assert_eq!(
			image.sample(Vec2::new(1.0, 0.5)),
			[0.0, 255.0, 0.0],
			"past the right edge"
		);
	}

	#[test]
	fn the_origin_of_the_texture_coordinates_is_at_the_bottom_left() {
		let image = Image {
			width: 1,
			height: 2,
			pixels: vec![[10, 10, 10], [200, 200, 200]],
		};
		assert_eq!(image.sample(Vec2::new(0.5, 0.0))[0], 200.0, "v of 0 is the bottom row");
		assert_eq!(image.sample(Vec2::new(0.5, 1.0))[0], 10.0, "v of 1 is the top row");
	}
}

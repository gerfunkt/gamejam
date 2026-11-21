//! The height grid: the ground of the arena as heights on a square grid, seen from above.
//!
//! The grid covers the square of [`tuning::TERRAIN_SIDE`] metres around the origin. Its first and last
//! points lie on the edges of the square. The file format is the single definition that the bake and
//! the rules share.
//!
//! A file holds a header and then the heights, row by row, with the row of the lowest Z first and the
//! column of the lowest X first. All numbers are little-endian:
//!
//! | bytes | content |
//! |---|---|
//! | 4 | the number of points on a side, as `u32` |
//! | 4 | the lowest height, in metres, as `f32` |
//! | 4 | the highest height, in metres, as `f32` |
//! | 2 for each point | the height as `u16`, from 0 for the lowest height to 65535 for the highest |

use std::fmt;

use crate::tuning;

/// The size of the header of a file, in bytes.
const HEADER: usize = 12;
/// The greatest value of a stored height.
const TOP: f32 = u16::MAX as f32;

/// Why the bytes of a height grid are not valid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridError {
	/// The data is shorter than the header, or its length does not fit the number of points.
	Length,
	/// The grid has fewer than two points on a side, so it has no cell.
	Side,
	/// The lowest height is not a number, or it is above the highest height.
	Range,
}

impl fmt::Display for GridError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str(match self {
			GridError::Length => "the length of the height grid does not fit its header",
			GridError::Side => "the height grid has fewer than two points on a side",
			GridError::Range => "the lowest height of the grid is not below its highest height",
		})
	}
}

impl std::error::Error for GridError {}

/// The heights of the ground at the points of a square grid.
#[derive(Clone, Debug, PartialEq)]
pub struct HeightGrid {
	side: usize,
	low: f32,
	high: f32,
	heights: Vec<f32>,
}

impl HeightGrid {
	/// Make a grid of `side` points on a side from `heights`, row by row, in metres.
	///
	/// The grid stores each height in 16 bits between the lowest and the highest of `heights`, so
	/// a height is exact to one 65535th of that range.
	///
	/// # Errors
	///
	/// Returns an error if `side` is below 2, if `heights` does not hold `side` times `side` numbers,
	/// or if a height is not a number.
	pub fn from_heights(side: usize, heights: &[f32]) -> Result<Self, GridError> {
		if side < 2 {
			return Err(GridError::Side);
		}
		if heights.len() != side * side {
			return Err(GridError::Length);
		}
		if heights.iter().any(|height| !height.is_finite()) {
			return Err(GridError::Range);
		}
		let low = heights.iter().copied().fold(f32::INFINITY, f32::min);
		let high = heights.iter().copied().fold(f32::NEG_INFINITY, f32::max);
		let levels: Vec<u16> = heights.iter().map(|&height| quantise(height, low, high)).collect();
		Ok(Self::from_levels(side, low, high, &levels))
	}

	/// Read a grid from the bytes of a file.
	///
	/// # Errors
	///
	/// Returns an error if the header or the length is wrong.
	pub fn from_bytes(bytes: &[u8]) -> Result<Self, GridError> {
		let (header, data) = bytes.split_first_chunk::<HEADER>().ok_or(GridError::Length)?;
		let word = |at: usize| [header[at], header[at + 1], header[at + 2], header[at + 3]];
		let side = usize::try_from(u32::from_le_bytes(word(0))).map_err(|_| GridError::Side)?;
		let (low, high) = (f32::from_le_bytes(word(4)), f32::from_le_bytes(word(8)));
		if side < 2 {
			return Err(GridError::Side);
		}
		if data.len()
			!= side
				.checked_mul(side)
				.and_then(|points| points.checked_mul(2))
				.ok_or(GridError::Length)?
		{
			return Err(GridError::Length);
		}
		if !(low <= high && high.is_finite()) {
			return Err(GridError::Range);
		}
		let levels: Vec<u16> = data
			.as_chunks::<2>()
			.0
			.iter()
			.map(|&pair| u16::from_le_bytes(pair))
			.collect();
		Ok(Self::from_levels(side, low, high, &levels))
	}

	fn from_levels(side: usize, low: f32, high: f32, levels: &[u16]) -> Self {
		let heights = levels
			.iter()
			.map(|&level| {
				let part = f32::from(level) / TOP;
				low * (1.0 - part) + high * part
			})
			.collect();
		Self {
			side,
			low,
			high,
			heights,
		}
	}

	/// Return the bytes of the file of this grid.
	#[must_use]
	pub fn to_bytes(&self) -> Vec<u8> {
		let mut bytes = Vec::with_capacity(HEADER + self.heights.len() * 2);
		bytes.extend_from_slice(&u32::try_from(self.side).unwrap_or(u32::MAX).to_le_bytes());
		bytes.extend_from_slice(&self.low.to_le_bytes());
		bytes.extend_from_slice(&self.high.to_le_bytes());
		for &height in &self.heights {
			bytes.extend_from_slice(&quantise(height, self.low, self.high).to_le_bytes());
		}
		bytes
	}

	/// Return the number of points on a side of the grid.
	#[must_use]
	pub fn side(&self) -> usize {
		self.side
	}

	/// Return the lowest height of the grid, in metres.
	#[must_use]
	pub fn low(&self) -> f32 {
		self.low
	}

	/// Return the highest height of the grid, in metres.
	#[must_use]
	pub fn high(&self) -> f32 {
		self.high
	}

	/// Return the height of the point in `column` and `row`, in metres.
	///
	/// Column 0 lies at the lowest X of the square, and row 0 at the lowest Z.
	///
	/// # Panics
	///
	/// Panics if the column or the row is not below [`HeightGrid::side`].
	#[must_use]
	pub fn point(&self, column: usize, row: usize) -> f32 {
		assert!(column < self.side && row < self.side, "the point lies outside the grid");
		self.heights[row * self.side + column]
	}

	/// Return the height of the ground at a place, in metres.
	///
	/// Between the four points around the place, the height is the even blend of them in both
	/// directions. Outside the square, it is the height at the nearest place on the edge.
	#[must_use]
	pub fn height(&self, x: f32, z: f32) -> f32 {
		let (column, row) = (self.coordinate(x), self.coordinate(z));
		let (first_column, first_row) = (self.cell(column), self.cell(row));
		let (across, down) = (column - first_column as f32, row - first_row as f32);
		let corner = |column: usize, row: usize| self.heights[row * self.side + column];
		let near = corner(first_column, first_row) * (1.0 - across) + corner(first_column + 1, first_row) * across;
		let far =
			corner(first_column, first_row + 1) * (1.0 - across) + corner(first_column + 1, first_row + 1) * across;
		near * (1.0 - down) + far * down
	}

	/// Return the highest point of the grid within `radius` metres of a place, in either direction.
	///
	/// The result reaches the corners of every cell that the square around the place touches, so
	/// it is never below the ground there. Outside the square, the edge counts.
	#[must_use]
	pub fn highest_near(&self, x: f32, z: f32, radius: f32) -> f32 {
		let (first_column, last_column) = (
			self.cell(self.coordinate(x - radius)),
			self.cell(self.coordinate(x + radius)) + 1,
		);
		let (first_row, last_row) = (
			self.cell(self.coordinate(z - radius)),
			self.cell(self.coordinate(z + radius)) + 1,
		);
		(first_row..=last_row)
			.flat_map(|row| (first_column..=last_column).map(move |column| (column, row)))
			.map(|(column, row)| self.heights[row * self.side + column])
			.fold(f32::NEG_INFINITY, f32::max)
	}

	/// Return the position of a world coordinate on the grid, in cells from the first point, held inside the grid.
	fn coordinate(&self, world: f32) -> f32 {
		let cells = (self.side - 1) as f32;
		((world / tuning::TERRAIN_SIDE + 0.5) * cells).clamp(0.0, cells)
	}

	/// Return the index of the cell that holds a position of the grid, so that the cell has a point on each side.
	fn cell(&self, position: f32) -> usize {
		(position as usize).min(self.side - 2)
	}
}

fn quantise(height: f32, low: f32, high: f32) -> u16 {
	if high > low {
		(((height - low) / (high - low)).clamp(0.0, 1.0) * TOP).round() as u16
	} else {
		0
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const SIDE: f32 = tuning::TERRAIN_SIDE;

	/// A grid of 3 by 3 points, at the corners, the edge middles, and the middle of the square.
	fn small() -> HeightGrid {
		HeightGrid::from_heights(3, &[0.0, 100.0, 200.0, 300.0, 400.0, 500.0, 600.0, 700.0, 800.0]).unwrap()
	}

	#[test]
	fn the_height_at_a_grid_point_is_the_height_of_that_point() {
		let grid = small();
		for row in 0..3 {
			for column in 0..3 {
				let (x, z) = ((column as f32 - 1.0) * SIDE / 2.0, (row as f32 - 1.0) * SIDE / 2.0);
				let wanted = (row * 3 + column) as f32 * 100.0;
				assert!(
					(grid.height(x, z) - wanted).abs() < 0.01,
					"the point {column} {row} should be {wanted} m, and is {}",
					grid.height(x, z)
				);
			}
		}
	}

	#[test]
	fn halfway_between_two_points_the_height_is_their_mean() {
		let grid = small();
		let across = grid.height(-SIDE / 4.0, -SIDE / 2.0);
		assert!((across - 50.0).abs() < 0.01, "between 0 and 100 m: {across}");
		let down = grid.height(-SIDE / 2.0, SIDE / 4.0);
		assert!((down - 450.0).abs() < 0.01, "between 300 and 600 m: {down}");
	}

	#[test]
	fn in_the_middle_of_four_points_the_height_is_their_mean() {
		let middle = small().height(-SIDE / 4.0, -SIDE / 4.0);
		assert!(
			(middle - 200.0).abs() < 0.01,
			"the mean of 0, 100, 300, and 400 m: {middle}"
		);
	}

	#[test]
	fn the_blend_is_even_along_a_line_between_two_points() {
		let grid = small();
		for step in 0..=10 {
			let part = step as f32 / 10.0;
			let height = grid.height(-SIDE / 2.0 + part * SIDE / 2.0, -SIDE / 2.0);
			assert!((height - part * 100.0).abs() < 0.01, "at {part} of the way: {height}");
		}
	}

	#[test]
	fn outside_the_square_the_height_is_the_height_of_the_nearest_edge_place() {
		let grid = small();
		assert_eq!(
			grid.height(-1.0e6, -1.0e6),
			grid.height(-SIDE / 2.0, -SIDE / 2.0),
			"past a corner"
		);
		assert_eq!(grid.height(1.0e6, 0.0), grid.height(SIDE / 2.0, 0.0), "past an edge");
		assert_eq!(
			grid.height(SIDE / 4.0, 1.0e6),
			grid.height(SIDE / 4.0, SIDE / 2.0),
			"past an edge, off a point"
		);
	}

	#[test]
	fn the_highest_point_near_a_place_reaches_every_cell_that_the_square_touches() {
		let grid = small();
		let corner = grid.highest_near(-SIDE / 2.0, -SIDE / 2.0, 1.0);
		assert!(
			(corner - 400.0).abs() < 0.02,
			"a corner touches only its own cell: {corner}"
		);
		let middle = grid.highest_near(0.0, 0.0, 1.0);
		assert!(
			(middle - 800.0).abs() < 0.02,
			"the middle touches all four cells: {middle}"
		);
		let all = grid.highest_near(0.0, 0.0, 1.0e6);
		assert!((all - 800.0).abs() < 0.02, "a huge radius covers the grid: {all}");
		let past = grid.highest_near(1.0e6, -1.0e6, 10.0);
		assert!(past >= grid.height(SIDE / 2.0, -SIDE / 2.0), "past a corner: {past}");
	}

	#[test]
	fn the_lowest_and_the_highest_height_are_kept_exactly() {
		let grid = small();
		assert_eq!((grid.low(), grid.high()), (0.0, 800.0), "the header range");
		assert_eq!((grid.point(0, 0), grid.point(2, 2)), (0.0, 800.0), "the end points");
	}

	#[test]
	fn the_header_holds_the_side_and_the_range_and_the_heights_follow_in_16_bits() {
		let bytes = small().to_bytes();
		assert_eq!(
			bytes.len(),
			12 + 9 * 2,
			"a header of 12 bytes and 2 bytes for each point"
		);
		assert_eq!(bytes[0..4], 3_u32.to_le_bytes(), "the side");
		assert_eq!(bytes[4..8], 0.0_f32.to_le_bytes(), "the lowest height");
		assert_eq!(bytes[8..12], 800.0_f32.to_le_bytes(), "the highest height");
		assert_eq!(bytes[12..14], [0, 0], "the lowest point is level 0");
		assert_eq!(bytes[28..30], [0xFF, 0xFF], "the highest point is level 65535");
	}

	#[test]
	fn a_grid_read_from_its_bytes_equals_the_grid_that_was_written() {
		let grid = small();
		assert_eq!(HeightGrid::from_bytes(&grid.to_bytes()).unwrap(), grid, "a round trip");
		let again = HeightGrid::from_bytes(&grid.to_bytes()).unwrap().to_bytes();
		assert_eq!(again, grid.to_bytes(), "the bytes are stable");
	}

	#[test]
	fn a_height_is_exact_to_one_level_of_the_range() {
		let heights: Vec<f32> = (0..16).map(|index| (index as f32).powi(2) * 1.37 + 0.3).collect();
		let grid = HeightGrid::from_heights(4, &heights).unwrap();
		let level = (grid.high() - grid.low()) / 65535.0;
		for (index, height) in heights.iter().enumerate() {
			assert!(
				(grid.point(index % 4, index / 4) - height).abs() <= level,
				"point {index} should be {height}, and is {}",
				grid.point(index % 4, index / 4)
			);
		}
	}

	#[test]
	fn bytes_that_do_not_fit_the_header_are_an_error() {
		let good = small().to_bytes();
		assert_eq!(
			HeightGrid::from_bytes(&good[..8]),
			Err(GridError::Length),
			"a cut header"
		);
		assert_eq!(
			HeightGrid::from_bytes(&good[..good.len() - 1]),
			Err(GridError::Length),
			"a cut body"
		);
		let mut thin = good.clone();
		thin[0..4].copy_from_slice(&1_u32.to_le_bytes());
		assert_eq!(
			HeightGrid::from_bytes(&thin),
			Err(GridError::Side),
			"a side of one point"
		);
		let mut upside_down = good.clone();
		upside_down[4..8].copy_from_slice(&900.0_f32.to_le_bytes());
		assert_eq!(
			HeightGrid::from_bytes(&upside_down),
			Err(GridError::Range),
			"low above high"
		);
		let mut huge = good;
		huge[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
		assert_eq!(
			HeightGrid::from_bytes(&huge),
			Err(GridError::Length),
			"a side that the data cannot hold"
		);
	}

	#[test]
	fn heights_that_do_not_make_a_square_or_are_not_numbers_are_an_error() {
		assert_eq!(HeightGrid::from_heights(1, &[0.0]), Err(GridError::Side), "one point");
		assert_eq!(
			HeightGrid::from_heights(2, &[0.0; 3]),
			Err(GridError::Length),
			"three of four"
		);
		assert_eq!(
			HeightGrid::from_heights(2, &[0.0, 1.0, f32::NAN, 2.0]),
			Err(GridError::Range),
			"a height that is not a number"
		);
	}

	#[test]
	fn a_flat_grid_has_a_range_of_zero_and_reads_back() {
		let grid = HeightGrid::from_heights(2, &[7.0; 4]).unwrap();
		assert_eq!(
			HeightGrid::from_bytes(&grid.to_bytes()).unwrap().height(0.0, 0.0),
			7.0,
			"all 7 m"
		);
	}
}

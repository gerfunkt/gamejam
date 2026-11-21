/// A small random number generator with a seed, so that a test can repeat a match.
///
/// It is SplitMix64. The rules need spread and variety from it, and no secrecy.
#[derive(Clone, Debug)]
pub(crate) struct Rng(u64);

impl Rng {
	pub(crate) fn new(seed: u64) -> Self {
		Self(seed)
	}

	fn next(&mut self) -> u64 {
		self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
		let mut z = self.0;
		z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
		z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
		z ^ (z >> 31)
	}

	/// Return a number from `low` up to, and not including, `high`.
	pub(crate) fn range(&mut self, low: f32, high: f32) -> f32 {
		// The top 24 bits fill the mantissa of an f32 exactly.
		let unit = (self.next() >> 40) as f32 / (1u64 << 24) as f32;
		low + (high - low) * unit
	}
}

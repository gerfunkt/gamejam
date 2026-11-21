//! A reader for the node tree of binary FBX files.
//!
//! The reader knows the file layout of FBX 7.x, versions 7100 to 7700. It does not know what the
//! nodes mean. [`crate::models::scene`] reads the model from the tree.

use anyhow::{Context, Result, bail, ensure};

const MAGIC: &[u8; 21] = b"Kaydara FBX Binary  \0";
/// From this version on, the offsets and counts of a node record have 64 bits.
const WIDE_VERSION: u32 = 7500;

/// One value in the property list of a node.
#[derive(Clone, Debug, PartialEq)]
pub enum Property {
	/// A boolean (type `C`).
	Bool(bool),
	/// A 16-bit integer (type `Y`).
	I16(i16),
	/// A 32-bit integer (type `I`).
	I32(i32),
	/// A 64-bit integer (type `L`).
	I64(i64),
	/// A 32-bit float (type `F`).
	F32(f32),
	/// A 64-bit float (type `D`).
	F64(f64),
	/// An array of 32-bit integers (type `i`).
	I32s(Vec<i32>),
	/// An array of 64-bit integers (type `l`).
	I64s(Vec<i64>),
	/// An array of 32-bit floats (type `f`).
	F32s(Vec<f32>),
	/// An array of 64-bit floats (type `d`).
	F64s(Vec<f64>),
	/// An array of booleans (type `b`).
	Bools(Vec<bool>),
	/// A string (type `S`). A 0x00 0x01 pair in it separates a name from its class.
	Str(String),
	/// Raw bytes (type `R`).
	Raw(Vec<u8>),
}

impl Property {
	/// Return this value as an integer, if it is a number that holds one.
	#[must_use]
	pub fn as_i64(&self) -> Option<i64> {
		match *self {
			Property::I16(value) => Some(value.into()),
			Property::I32(value) => Some(value.into()),
			Property::I64(value) => Some(value),
			_ => None,
		}
	}

	/// Return this value as a float, if it is a number.
	#[must_use]
	pub fn as_f64(&self) -> Option<f64> {
		match *self {
			Property::F32(value) => Some(value.into()),
			Property::F64(value) => Some(value),
			_ => self.as_i64().map(|value| value as f64),
		}
	}

	/// Return this value as a string, if it is one.
	#[must_use]
	pub fn as_str(&self) -> Option<&str> {
		match self {
			Property::Str(value) => Some(value),
			_ => None,
		}
	}
}

/// A node of the tree: a name, a list of values, and child nodes.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
	/// The name of the node, such as `Geometry`.
	pub name: String,
	/// The values of the node, in file order.
	pub properties: Vec<Property>,
	/// The nodes below this one, in file order.
	pub children: Vec<Node>,
}

impl Node {
	/// Return the first child with this name.
	#[must_use]
	pub fn child(&self, name: &str) -> Option<&Node> {
		self.children.iter().find(|child| child.name == name)
	}

	/// Return every child with this name.
	pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
		self.children.iter().filter(move |child| child.name == name)
	}
}

/// Parse a binary FBX file into its top-level nodes.
///
/// # Errors
///
/// Returns an error if the bytes are not a binary FBX file of version 7.x, or if a record reaches
/// beyond the end of the file.
pub fn parse(bytes: &[u8]) -> Result<Vec<Node>> {
	ensure!(bytes.starts_with(MAGIC), "not a binary FBX file: the header is missing");
	let mut reader = Reader {
		bytes,
		position: MAGIC.len() + 2,
	};
	let version = reader.u32()?;
	ensure!(
		(7100..=7700).contains(&version),
		"FBX version {version} is not supported; export version 7.4 or 7.5"
	);
	let mut nodes = Vec::new();
	while let Some(node) = reader.node(version)? {
		nodes.push(node);
	}
	Ok(nodes)
}

struct Reader<'a> {
	bytes: &'a [u8],
	position: usize,
}

impl Reader<'_> {
	fn take(&mut self, length: usize) -> Result<&[u8]> {
		let end = self.position.checked_add(length).filter(|end| *end <= self.bytes.len());
		let end = end.with_context(|| format!("the file ends inside a record at byte {}", self.position))?;
		let slice = &self.bytes[self.position..end];
		self.position = end;
		Ok(slice)
	}

	fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
		let mut array = [0; N];
		array.copy_from_slice(self.take(N)?);
		Ok(array)
	}

	fn u8(&mut self) -> Result<u8> {
		Ok(self.array::<1>()?[0])
	}

	fn u32(&mut self) -> Result<u32> {
		Ok(u32::from_le_bytes(self.array()?))
	}

	fn u64(&mut self) -> Result<u64> {
		Ok(u64::from_le_bytes(self.array()?))
	}

	fn length(&mut self) -> Result<usize> {
		Ok(usize::try_from(self.u32()?)?)
	}

	/// Read one node record. Return `None` at the null record that ends a list of nodes.
	fn node(&mut self, version: u32) -> Result<Option<Node>> {
		let wide = version >= WIDE_VERSION;
		let (end, count, list_length) = if wide {
			(self.u64()?, self.u64()?, self.u64()?)
		} else {
			(self.u32()?.into(), self.u32()?.into(), self.u32()?.into())
		};
		let name_length = usize::from(self.u8()?);
		if end == 0 {
			ensure!(
				count == 0 && list_length == 0 && name_length == 0,
				"a malformed end record"
			);
			return Ok(None);
		}
		let end = usize::try_from(end)?;
		ensure!(end <= self.bytes.len(), "a node record ends beyond the end of the file");
		let name = String::from_utf8_lossy(self.take(name_length)?).into_owned();
		let properties_end = self.position + usize::try_from(list_length)?;
		let mut properties = Vec::new();
		for _ in 0..count {
			properties.push(self.property()?);
		}
		ensure!(
			self.position == properties_end,
			"the property list of `{name}` has the wrong length"
		);
		let mut children = Vec::new();
		while self.position < end {
			match self.node(version)? {
				Some(child) => children.push(child),
				None => break,
			}
		}
		ensure!(
			self.position == end,
			"the node `{name}` does not end where its record says"
		);
		Ok(Some(Node {
			name,
			properties,
			children,
		}))
	}

	fn property(&mut self) -> Result<Property> {
		let code = self.u8()?;
		Ok(match code {
			b'C' => Property::Bool(self.u8()? & 1 == 1),
			b'Y' => Property::I16(i16::from_le_bytes(self.array()?)),
			b'I' => Property::I32(i32::from_le_bytes(self.array()?)),
			b'L' => Property::I64(i64::from_le_bytes(self.array()?)),
			b'F' => Property::F32(f32::from_le_bytes(self.array()?)),
			b'D' => Property::F64(f64::from_le_bytes(self.array()?)),
			b'S' => {
				let length = self.length()?;
				Property::Str(String::from_utf8_lossy(self.take(length)?).into_owned())
			}
			b'R' => {
				let length = self.length()?;
				Property::Raw(self.take(length)?.to_vec())
			}
			b'b' => Property::Bools(self.array_data(1)?.iter().map(|byte| byte & 1 == 1).collect()),
			b'i' => Property::I32s(chunks(&self.array_data(4)?, i32::from_le_bytes)),
			b'l' => Property::I64s(chunks(&self.array_data(8)?, i64::from_le_bytes)),
			b'f' => Property::F32s(chunks(&self.array_data(4)?, f32::from_le_bytes)),
			b'd' => Property::F64s(chunks(&self.array_data(8)?, f64::from_le_bytes)),
			other => bail!("unknown property type {:?}", char::from(other)),
		})
	}

	/// Read the header and the data of an array, and inflate the data if the file packs it.
	fn array_data(&mut self, element_size: usize) -> Result<Vec<u8>> {
		let count = self.length()?;
		let encoding = self.u32()?;
		let stored = self.length()?;
		let data = self.take(stored)?;
		let data = match encoding {
			0 => data.to_vec(),
			1 => miniz_oxide::inflate::decompress_to_vec_zlib(data)
				.map_err(|error| anyhow::anyhow!("an array does not inflate: {error:?}"))?,
			other => bail!("unknown array encoding {other}"),
		};
		ensure!(
			Some(data.len()) == count.checked_mul(element_size),
			"an array has {} bytes, but {count} elements of {element_size} bytes",
			data.len()
		);
		Ok(data)
	}
}

fn chunks<T, const N: usize>(data: &[u8], convert: fn([u8; N]) -> T) -> Vec<T> {
	data.chunks_exact(N)
		.map(|chunk| convert(chunk.try_into().expect("chunks_exact gives chunks of N bytes")))
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Encode a node record of version 7.4, the way an exporter does.
	pub(crate) fn encode_node(
		out: &mut Vec<u8>,
		name: &str,
		properties: &[Vec<u8>],
		children: &[&dyn Fn(&mut Vec<u8>)],
	) {
		let start = out.len();
		out.extend_from_slice(&[0; 12]);
		out.push(u8::try_from(name.len()).unwrap());
		out.extend_from_slice(name.as_bytes());
		let list_start = out.len();
		for property in properties {
			out.extend_from_slice(property);
		}
		let list_length = out.len() - list_start;
		for child in children {
			child(out);
		}
		if !children.is_empty() {
			out.extend_from_slice(&[0; 13]);
		}
		let end = u32::try_from(out.len()).unwrap();
		out[start..start + 4].copy_from_slice(&end.to_le_bytes());
		out[start + 4..start + 8].copy_from_slice(&u32::try_from(properties.len()).unwrap().to_le_bytes());
		out[start + 8..start + 12].copy_from_slice(&u32::try_from(list_length).unwrap().to_le_bytes());
	}

	pub(crate) fn file(body: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
		let mut out = MAGIC.to_vec();
		out.extend_from_slice(&[0x1a, 0]);
		out.extend_from_slice(&7400_u32.to_le_bytes());
		body(&mut out);
		out.extend_from_slice(&[0; 13]);
		out
	}

	pub(crate) fn int(value: i32) -> Vec<u8> {
		[&b"I"[..], &value.to_le_bytes()].concat()
	}

	pub(crate) fn text(value: &str) -> Vec<u8> {
		[
			&b"S"[..],
			&u32::try_from(value.len()).unwrap().to_le_bytes(),
			value.as_bytes(),
		]
		.concat()
	}

	fn array(code: u8, count: usize, packed: bool, data: &[u8]) -> Vec<u8> {
		let stored = if packed {
			miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
		} else {
			data.to_vec()
		};
		let mut out = vec![code];
		out.extend_from_slice(&u32::try_from(count).unwrap().to_le_bytes());
		out.extend_from_slice(&u32::from(packed).to_le_bytes());
		out.extend_from_slice(&u32::try_from(stored.len()).unwrap().to_le_bytes());
		out.extend_from_slice(&stored);
		out
	}

	pub(crate) fn doubles(values: &[f64], packed: bool) -> Vec<u8> {
		let data: Vec<u8> = values.iter().flat_map(|value| value.to_le_bytes()).collect();
		array(b'd', values.len(), packed, &data)
	}

	pub(crate) fn ints(values: &[i32], packed: bool) -> Vec<u8> {
		let data: Vec<u8> = values.iter().flat_map(|value| value.to_le_bytes()).collect();
		array(b'i', values.len(), packed, &data)
	}

	#[test]
	fn a_node_with_scalar_properties_and_a_child_is_read() {
		let bytes = file(|out| {
			encode_node(
				out,
				"Outer",
				&[int(7), text("name")],
				&[&|out| encode_node(out, "Inner", &[], &[])],
			);
		});
		let nodes = parse(&bytes).unwrap();
		assert_eq!(nodes.len(), 1, "one top-level node");
		assert_eq!(nodes[0].name, "Outer");
		assert_eq!(nodes[0].properties, [Property::I32(7), Property::Str("name".into())]);
		assert_eq!(nodes[0].children[0].name, "Inner");
	}

	#[test]
	fn a_packed_array_is_inflated_to_its_elements() {
		let values = [1.5, -2.0, 3.25];
		let bytes = file(|out| encode_node(out, "Vertices", &[doubles(&values, true)], &[]));
		assert_eq!(parse(&bytes).unwrap()[0].properties, [Property::F64s(values.to_vec())]);
	}

	#[test]
	fn an_unpacked_array_is_read_as_it_is() {
		let bytes = file(|out| encode_node(out, "Indices", &[ints(&[0, 1, -3], false)], &[]));
		assert_eq!(parse(&bytes).unwrap()[0].properties, [Property::I32s(vec![0, 1, -3])]);
	}

	#[test]
	fn a_file_without_the_header_is_refused() {
		let error = parse(b"not an fbx file at all, but long enough").unwrap_err();
		assert!(
			error.to_string().contains("not a binary FBX"),
			"the message names the problem: {error}"
		);
	}

	#[test]
	fn a_file_that_ends_inside_a_record_is_refused() {
		let mut bytes = file(|out| encode_node(out, "Vertices", &[doubles(&[1.0, 2.0], false)], &[]));
		bytes.truncate(bytes.len() - 30);
		assert!(parse(&bytes).is_err(), "a cut file is an error, not a panic");
	}
}

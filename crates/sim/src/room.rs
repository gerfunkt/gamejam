//! The frames of the message bus that are not game messages.
//!
//! The bus holds rooms. A room sends each binary message to every member, the sender included,
//! in one order, with the number of the sender in front. The bus never reads the rest of a
//! binary message. Text frames carry the state of the room and the requests of its owner. The
//! server writes these frames with this module, and the client reads them with it, so both
//! sides share one definition.

use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use serde::{Deserialize, Serialize};

/// The most members that one room holds.
pub const MAX_MEMBERS: usize = 16;
/// The most rooms that one server holds, the open room included.
pub const MAX_ROOMS: usize = 64;
/// The largest message that the bus passes on, in bytes. The bus drops a larger one.
pub const MAX_MESSAGE_BYTES: usize = 4096;
/// The most characters of a room title.
pub const MAX_TITLE_CHARS: usize = 32;
/// The most characters of the info line of a room.
pub const MAX_INFO_CHARS: usize = 120;
/// The id of the open room, which always exists.
pub const OPEN_ROOM_ID: &str = "0";
/// The title of the open room.
pub const OPEN_ROOM_TITLE: &str = "Open room";

const STATUS_PREFIX: &str = "room:";

/// The state of a room, as the bus tells it to one member.
///
/// The bus sends it when the member joins, and again when the members, the owner, or the title
/// change. It takes its place in the order of the room, between the binary messages.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomStatus {
	/// The id of the room: a decimal number that the server counts up.
	pub id: String,
	/// The title of the room.
	pub title: String,
	/// The number of the member that owns the room: the one with the lowest number.
	pub owner: u32,
	/// The numbers of the members, in the order in which they joined.
	pub members: Vec<u32>,
	/// The number of the member that receives this status.
	pub you: u32,
}

impl RoomStatus {
	/// Return the text frame that carries this status.
	#[must_use]
	pub fn to_frame(&self) -> String {
		let json = serde_json::to_string(self).expect("a room status has only strings and numbers");
		format!("{STATUS_PREFIX}{json}")
	}

	/// Read a status from a text frame. Any other text gives `None`.
	#[must_use]
	pub fn parse_frame(text: &str) -> Option<Self> {
		serde_json::from_str(text.strip_prefix(STATUS_PREFIX)?).ok()
	}
}

/// One room in the list that the server gives at `/rooms`, in the order of creation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomEntry {
	/// The id of the room, which joins it.
	pub id: String,
	/// The title of the room.
	pub title: String,
	/// The number of members in the room.
	pub members: usize,
	/// The number of the owner. An empty room has none.
	pub owner: Option<u32>,
	/// The line in which the owner says what the room does.
	pub info: String,
}

/// Return the body of `/rooms` for `rooms`.
#[must_use]
pub fn encode_room_list(rooms: &[RoomEntry]) -> String {
	serde_json::to_string(rooms).expect("a room entry has only strings and numbers")
}

/// Read the body of `/rooms`.
///
/// # Errors
///
/// Returns the JSON error when `body` is not a room list.
pub fn parse_room_list(body: &str) -> Result<Vec<RoomEntry>, serde_json::Error> {
	serde_json::from_str(body)
}

/// A request of the owner of a room to the bus. The bus never passes it on to the members.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Control {
	/// Set the info line of the room.
	Info(String),
	/// Rename the room.
	Title(String),
	/// Close the connection of the member with this number.
	Kick(u32),
}

impl Control {
	/// Return the text frame that carries this request.
	#[must_use]
	pub fn to_frame(&self) -> String {
		match self {
			Control::Info(text) => format!("info:{text}"),
			Control::Title(text) => format!("title:{text}"),
			Control::Kick(number) => format!("kick:{number}"),
		}
	}

	/// Read a request from a text frame. Any other text gives `None`.
	#[must_use]
	pub fn parse_frame(text: &str) -> Option<Self> {
		let (kind, rest) = text.split_once(':')?;
		match kind {
			"info" => Some(Control::Info(rest.to_owned())),
			"title" => Some(Control::Title(rest.to_owned())),
			"kick" => rest.parse().ok().map(Control::Kick),
			_ => None,
		}
	}
}

/// How a player enters the bus, as the query of the address `/ws`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Join {
	/// Join the open room.
	Open,
	/// Join the room with this id.
	Room(String),
	/// Create a room with this title, and join it.
	Create(String),
}

impl Join {
	/// Return the query for this entry, with its leading `?`. The open room needs no query.
	#[must_use]
	pub fn to_query(&self) -> String {
		let encode = |text: &str| utf8_percent_encode(text, NON_ALPHANUMERIC).to_string();
		match self {
			Join::Open => String::new(),
			Join::Room(id) => format!("?room={}", encode(id)),
			Join::Create(title) => format!("?create={}", encode(title)),
		}
	}

	/// Read the entry from the query of a request, without its leading `?`.
	///
	/// A query that names neither a room nor a title joins the open room.
	#[must_use]
	pub fn parse_query(query: Option<&str>) -> Self {
		let decode = |text: &str| percent_decode_str(text).decode_utf8_lossy().into_owned();
		for pair in query.unwrap_or_default().split('&') {
			match pair.split_once('=') {
				Some(("room", id)) => return Join::Room(decode(id)),
				Some(("create", title)) => return Join::Create(decode(title)),
				_ => {}
			}
		}
		Join::Open
	}
}

/// The reason for which the bus closes a connection, as the code of the close frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseCode {
	/// The owner of the room removed the member.
	Removed = 4000,
	/// No room has the id of the request.
	NoSuchRoom = 4001,
	/// The room already holds [`MAX_MEMBERS`].
	RoomFull = 4002,
	/// The server already holds [`MAX_ROOMS`].
	TooManyRooms = 4003,
	/// The member fell too far behind in reading, and the bus removed it.
	FellBehind = 4004,
}

impl CloseCode {
	/// Return the number of this code in a close frame.
	#[must_use]
	pub fn code(self) -> u16 {
		self as u16
	}

	/// Return the reason text of the close frame.
	#[must_use]
	pub fn reason(self) -> &'static str {
		match self {
			CloseCode::Removed => "removed by the host",
			CloseCode::NoSuchRoom => "no such room",
			CloseCode::RoomFull => "room is full",
			CloseCode::TooManyRooms => "too many rooms",
			CloseCode::FellBehind => "fell behind",
		}
	}

	/// Return the code with this number. A number that the bus does not use gives `None`.
	#[must_use]
	pub fn from_code(code: u16) -> Option<Self> {
		[
			CloseCode::Removed,
			CloseCode::NoSuchRoom,
			CloseCode::RoomFull,
			CloseCode::TooManyRooms,
			CloseCode::FellBehind,
		]
		.into_iter()
		.find(|known| known.code() == code)
	}
}

/// Return the title that a room with `id` gets from the text `raw` of a player.
///
/// The title is trimmed and cut to [`MAX_TITLE_CHARS`]. An empty title becomes "Room" and the id.
#[must_use]
pub fn clean_title(raw: &str, id: &str) -> String {
	let title: String = raw.trim().chars().take(MAX_TITLE_CHARS).collect();
	if title.is_empty() { format!("Room {id}") } else { title }
}

/// Return the info line that a room gets from the text `raw` of its owner, cut to [`MAX_INFO_CHARS`].
#[must_use]
pub fn clean_info(raw: &str) -> String {
	raw.trim().chars().take(MAX_INFO_CHARS).collect()
}

/// Return the binary frame that the bus sends for a message of the member `sender`.
///
/// The frame starts with the number of the sender, as four bytes with the lowest first. The
/// message follows unchanged.
#[must_use]
pub fn frame(sender: u32, message: &[u8]) -> Vec<u8> {
	[&sender.to_le_bytes()[..], message].concat()
}

/// Split a binary frame of the bus into the number of the sender and the message.
///
/// A frame that is too short for a number gives `None`.
#[must_use]
pub fn split_frame(frame: &[u8]) -> Option<(u32, &[u8])> {
	let (number, message) = frame.split_first_chunk::<4>()?;
	Some((u32::from_le_bytes(*number), message))
}

//! The frames of the bus, checked against the examples of `design.md`.

use sim::room::{
	CloseCode, Control, Join, MAX_INFO_CHARS, MAX_TITLE_CHARS, RoomEntry, RoomStatus, clean_info, clean_title,
	encode_room_list, frame, parse_room_list, split_frame,
};

fn status() -> RoomStatus {
	RoomStatus {
		id: "3".into(),
		title: "Friday".into(),
		owner: 12,
		members: vec![12, 15, 16],
		you: 15,
	}
}

#[test]
fn a_room_status_has_the_documented_text() {
	assert_eq!(
		status().to_frame(),
		r#"room:{"id":"3","title":"Friday","owner":12,"members":[12,15,16],"you":15}"#
	);
}

#[test]
fn a_room_status_survives_the_frame() {
	let quoted = RoomStatus {
		title: "\"Zoë\" \\ 北".into(),
		..status()
	};
	assert_eq!(RoomStatus::parse_frame(&quoted.to_frame()), Some(quoted));
}

#[test]
fn other_text_is_not_a_room_status() {
	for text in ["", "room:", "room:{}", "title:x", r#"{"id":"3"}"#] {
		assert_eq!(RoomStatus::parse_frame(text), None, "{text:?} was read as a status");
	}
}

#[test]
fn a_room_list_has_the_documented_text() {
	let rooms = [
		RoomEntry {
			id: "0".into(),
			title: "Open room".into(),
			members: 3,
			owner: Some(12),
			info: "In a match".into(),
		},
		RoomEntry {
			id: "4".into(),
			title: "Empty".into(),
			members: 0,
			owner: None,
			info: String::new(),
		},
	];
	let body = encode_room_list(&rooms);
	assert_eq!(
		body,
		r#"[{"id":"0","title":"Open room","members":3,"owner":12,"info":"In a match"},{"id":"4","title":"Empty","members":0,"owner":null,"info":""}]"#
	);
	assert_eq!(parse_room_list(&body).expect("the list must parse"), rooms);
}

#[test]
fn a_text_that_is_no_room_list_is_an_error() {
	assert!(parse_room_list("<html>").is_err());
}

#[test]
fn each_control_request_survives_its_frame() {
	let requests = [
		Control::Info("3 bots: Ace".into()),
		Control::Title("a: b".into()),
		Control::Kick(15),
	];
	for request in requests {
		assert_eq!(Control::parse_frame(&request.to_frame()), Some(request));
	}
	assert_eq!(Control::Kick(15).to_frame(), "kick:15");
}

#[test]
fn other_text_is_not_a_control_request() {
	for text in ["", "kick:", "kick:bob", "kick:-1", "start", "room:{}"] {
		assert_eq!(Control::parse_frame(text), None, "{text:?} was read as a request");
	}
}

#[test]
fn each_way_into_the_bus_survives_its_query() {
	for join in [
		Join::Open,
		Join::Room("17".into()),
		Join::Create("Zoë's room & more = 100%".into()),
	] {
		let query = join.to_query();
		assert_eq!(
			Join::parse_query(query.strip_prefix('?')),
			join,
			"through the query {query:?}"
		);
	}
	assert_eq!(Join::Room("17".into()).to_query(), "?room=17");
	assert_eq!(
		Join::parse_query(Some("create=My%20room")),
		Join::Create("My room".into())
	);
}

#[test]
fn a_query_without_a_room_joins_the_open_room() {
	for query in [None, Some(""), Some("auto=online"), Some("room")] {
		assert_eq!(Join::parse_query(query), Join::Open, "for the query {query:?}");
	}
}

#[test]
fn a_title_is_trimmed_and_cut_and_never_empty() {
	assert_eq!(clean_title("  Friday night  ", "3"), "Friday night");
	assert_eq!(clean_title("   ", "3"), "Room 3");
	let long = "é".repeat(MAX_TITLE_CHARS + 10);
	assert_eq!(clean_title(&long, "3").chars().count(), MAX_TITLE_CHARS);
}

#[test]
fn an_info_line_is_cut() {
	assert_eq!(clean_info(&"x".repeat(MAX_INFO_CHARS + 1)).len(), MAX_INFO_CHARS);
}

#[test]
fn each_close_code_has_the_documented_number_and_reason() {
	let documented = [
		(CloseCode::Removed, 4000, "removed by the host"),
		(CloseCode::NoSuchRoom, 4001, "no such room"),
		(CloseCode::RoomFull, 4002, "room is full"),
		(CloseCode::TooManyRooms, 4003, "too many rooms"),
	];
	for (code, number, reason) in documented {
		assert_eq!(code.code(), number);
		assert_eq!(code.reason(), reason);
		assert_eq!(CloseCode::from_code(number), Some(code));
	}
	assert_eq!(CloseCode::from_code(1000), None);
}

#[test]
fn a_binary_frame_carries_the_sender_in_front_of_the_message() {
	let sent = frame(0x0102_0304, b"state");
	assert_eq!(sent, [4, 3, 2, 1, b's', b't', b'a', b't', b'e']);
	assert_eq!(split_frame(&sent), Some((0x0102_0304, &b"state"[..])));
	assert_eq!(split_frame(&[1, 2, 3]), None, "three bytes cannot hold a sender");
	assert_eq!(split_frame(&[9, 0, 0, 0]), Some((9, &[][..])));
}

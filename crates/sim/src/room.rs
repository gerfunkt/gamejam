//! The text frames of the bus: room status, control requests, and
//! close codes. The server relays none of them; it enforces them.

use serde::Deserialize;

/// The status of a room, sent to each member on every change.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Status {
    pub id: String,
    pub title: String,
    pub owner: Option<u32>,
    pub members: Vec<u32>,
    pub you: u32,
}

/// One entry of the room list at GET /rooms.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Listed {
    pub id: String,
    pub title: String,
    pub members: u32,
    pub owner: Option<u32>,
    #[serde(default)]
    pub info: Option<String>,
}

pub const OPEN_ROOM: &str = "0";

pub mod close {
    pub const KICKED: u16 = 4000;
    pub const NO_SUCH_ROOM: u16 = 4001;
    pub const ROOM_FULL: u16 = 4002;
    pub const TOO_MANY_ROOMS: u16 = 4003;
}

pub fn parse_status(text: &str) -> Option<Status> {
    serde_json::from_str(text.strip_prefix("room:")?).ok()
}

pub fn parse_list(body: &str) -> Vec<Listed> {
    serde_json::from_str(body).unwrap_or_default()
}

/// Control frames from the owner of a room.
pub fn info(text: &str) -> String {
    format!("info:{text}")
}

pub fn title(text: &str) -> String {
    format!("title:{text}")
}

pub fn kick(num: u32) -> String {
    format!("kick:{num}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_status_frame() {
        let status =
            parse_status(r#"room:{"id":"3","title":"Dogfight","owner":12,"members":[12,15,16],"you":15}"#)
                .expect("status");
        assert_eq!(status.id, "3");
        assert_eq!(status.title, "Dogfight");
        assert_eq!(status.owner, Some(12));
        assert_eq!(status.members, vec![12, 15, 16]);
        assert_eq!(status.you, 15);
    }

    #[test]
    fn rejects_other_frames() {
        assert!(parse_status("info:hello").is_none());
        assert!(parse_status("room:{not json").is_none());
        assert!(parse_status("").is_none());
    }

    #[test]
    fn parses_the_room_list() {
        let list = parse_list(
            r#"[{"id":"0","title":"Open room","members":3,"owner":12,"info":"hi"},{"id":"1","title":"A","members":0,"owner":null}]"#,
        );
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].title, "Open room");
        assert_eq!(list[0].info.as_deref(), Some("hi"));
        assert_eq!(list[1].owner, None);
    }
}

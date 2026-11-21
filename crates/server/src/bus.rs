//! The message bus with rooms, as a pure state machine: no TLS, no
//! HTTP, no sockets. The HTTP layer maps it onto WebSocket frames.

use std::collections::BTreeMap;

pub const OPEN_ROOM: u64 = 0;
pub const MAX_MEMBERS: usize = 16;
pub const MAX_ROOMS: usize = 64;
pub const MAX_MESSAGE: usize = 4 * 1024;

pub mod close {
    pub const KICKED: u16 = 4000;
    pub const NO_SUCH_ROOM: u16 = 4001;
    pub const ROOM_FULL: u16 = 4002;
    pub const TOO_MANY_ROOMS: u16 = 4003;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    Binary(Vec<u8>),
    Text(String),
    Close(u16, String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Out {
    pub to: u32,
    pub frame: Frame,
}

#[derive(Clone, Debug, Default)]
pub struct Room {
    pub title: String,
    pub info: Option<String>,
    pub members: BTreeMap<u32, ()>,
}

impl Room {
    fn owner(&self) -> Option<u32> {
        self.members.keys().next().copied()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Join {
    Open,
    Existing(u64),
    Create(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub id: u64,
    pub title: String,
    pub members: usize,
    pub owner: Option<u32>,
    pub info: Option<String>,
}

#[derive(Default)]
pub struct Bus {
    rooms: BTreeMap<u64, Room>,
    next_room: u64,
    next_player: u32,
    /// The room each player is in.
    placement: BTreeMap<u32, u64>,
}

impl Bus {
    pub fn new() -> Self {
        let mut bus = Self::default();
        bus.rooms.insert(OPEN_ROOM, Room {
            title: "Open room".into(),
            ..Room::default()
        });
        bus.next_room = 1;
        bus
    }

    pub fn join(&mut self, want: Join) -> Result<(u32, Vec<Out>), (u16, &'static str)> {
        let room_id = match want {
            Join::Open => OPEN_ROOM,
            Join::Existing(id) => {
                if !self.rooms.contains_key(&id) {
                    return Err((close::NO_SUCH_ROOM, "no such room"));
                }
                id
            }
            Join::Create(title) => {
                if self.rooms.len() >= MAX_ROOMS {
                    return Err((close::TOO_MANY_ROOMS, "too many rooms"));
                }
                let id = self.next_room;
                self.next_room += 1;
                let title = clean_title(&title)
                    .unwrap_or_else(|| format!("Room {id}"));
                self.rooms.insert(id, Room { title, ..Room::default() });
                id
            }
        };
        let room = self.rooms.get_mut(&room_id).unwrap();
        if room.members.len() >= MAX_MEMBERS {
            return Err((close::ROOM_FULL, "room is full"));
        }
        self.next_player += 1;
        let num = self.next_player;
        room.members.insert(num, ());
        self.placement.insert(num, room_id);
        Ok((num, self.status(room_id)))
    }

    pub fn leave(&mut self, num: u32) -> Vec<Out> {
        let Some(room_id) = self.placement.remove(&num) else {
            return Vec::new();
        };
        let room = self.rooms.get_mut(&room_id).unwrap();
        room.members.remove(&num);
        if room.members.is_empty() && room_id != OPEN_ROOM {
            self.rooms.remove(&room_id);
            return Vec::new();
        }
        self.status(room_id)
    }

    /// A binary message: relayed to every member of the room, the
    /// number of the sender in front, in one order. Too large is dropped.
    pub fn binary(&mut self, from: u32, bytes: &[u8]) -> Vec<Out> {
        if bytes.len() > MAX_MESSAGE {
            return Vec::new();
        }
        let Some(room_id) = self.placement.get(&from).copied() else {
            return Vec::new();
        };
        let mut payload = Vec::with_capacity(4 + bytes.len());
        payload.extend_from_slice(&from.to_le_bytes());
        payload.extend_from_slice(bytes);
        self.rooms[&room_id]
            .members
            .keys()
            .map(|to| Out { to: *to, frame: Frame::Binary(payload.clone()) })
            .collect()
    }

    /// A text frame. Only the owner's control requests act; the server
    /// relays no text and ignores the rest.
    pub fn text(&mut self, from: u32, text: &str) -> Vec<Out> {
        let Some(room_id) = self.placement.get(&from).copied() else {
            return Vec::new();
        };
        if self.rooms[&room_id].owner() != Some(from) {
            return Vec::new();
        }
        if let Some(info) = text.strip_prefix("info:") {
            let mut info = info.trim().to_string();
            truncate_chars(&mut info, 120);
            self.rooms.get_mut(&room_id).unwrap().info = (!info.is_empty()).then_some(info);
            Vec::new()
        } else if let Some(title) = text.strip_prefix("title:") {
            if let Some(title) = clean_title(title) {
                self.rooms.get_mut(&room_id).unwrap().title = title;
                return self.status(room_id);
            }
            Vec::new()
        } else if let Some(num) = text.strip_prefix("kick:") {
            let Ok(num) = num.trim().parse::<u32>() else {
                return Vec::new();
            };
            if num == from || !self.rooms[&room_id].members.contains_key(&num) {
                return Vec::new();
            }
            let close = Out {
                to: num,
                frame: Frame::Close(close::KICKED, "removed by the host".into()),
            };
            let mut out = vec![close];
            out.extend(self.leave(num));
            out
        } else {
            Vec::new()
        }
    }

    /// The room status frame for each member, with its own number.
    fn status(&self, room_id: u64) -> Vec<Out> {
        let room = &self.rooms[&room_id];
        room.members
            .keys()
            .map(|you| Out {
                to: *you,
                frame: Frame::Text(format!(
                    "room:{{\"id\":\"{room_id}\",\"title\":{},\"owner\":{},\"members\":[{}],\"you\":{you}}}",
                    json_string(&room.title),
                    room.owner().map(|n| n.to_string()).unwrap_or_else(|| "null".into()),
                    room.members
                        .keys()
                        .map(|n| n.to_string())
                        .collect::<Vec<_>>()
                        .join(","),
                )),
            })
            .collect()
    }

    /// The room list, in the order of creation.
    pub fn list(&self) -> Vec<Listed> {
        self.rooms
            .iter()
            .map(|(id, room)| Listed {
                id: *id,
                title: room.title.clone(),
                members: room.members.len(),
                owner: room.owner(),
                info: room.info.clone(),
            })
            .collect()
    }
}

/// Trim, and cut to 32 characters. Empty titles become None.
pub fn clean_title(raw: &str) -> Option<String> {
    let mut title = raw.trim().to_string();
    truncate_chars(&mut title, 32);
    (!title.is_empty()).then_some(title)
}

fn truncate_chars(text: &mut String, max: usize) {
    if text.chars().count() > max {
        *text = text.chars().take(max).collect();
    }
}

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(outs: &[Out]) -> Vec<&str> {
        outs.iter()
            .filter_map(|o| match &o.frame {
                Frame::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn join_assigns_numbers_and_sends_status() {
        let mut bus = Bus::new();
        let (a, out_a) = bus.join(Join::Open).unwrap();
        let (b, out_b) = bus.join(Join::Open).unwrap();
        assert_eq!(b, a + 1, "player numbers count up");
        let status = texts(&out_b);
        assert_eq!(status.len(), 2, "both members get the new status");
        assert!(status[0].contains("\"owner\":1"));
        assert!(status[1].contains("\"you\":2"));
        assert!(out_a.iter().all(|o| o.to == a), "the first join tells only the joiner");
    }

    #[test]
    fn create_and_join_a_room() {
        let mut bus = Bus::new();
        let (a, _) = bus.join(Join::Create("  Dogfight  ".into())).unwrap();
        let listed = bus.list();
        let room = listed.iter().find(|r| r.title == "Dogfight").unwrap();
        let (b, _) = bus.join(Join::Existing(room.id)).unwrap();
        assert_ne!(room.id, OPEN_ROOM);
        assert_eq!(bus.list().iter().find(|r| r.id == room.id).unwrap().members, 2);
        assert_eq!(bus.list().iter().find(|r| r.id == room.id).unwrap().owner, Some(a.min(b)));
    }

    #[test]
    fn a_missing_room_is_refused() {
        let mut bus = Bus::new();
        assert_eq!(bus.join(Join::Existing(99)), Err((close::NO_SUCH_ROOM, "no such room")));
    }

    #[test]
    fn a_full_room_is_refused() {
        let mut bus = Bus::new();
        let (_, outs) = bus.join(Join::Create("full".into())).unwrap();
        let _ = outs;
        let id = bus.list()[1].id;
        for _ in 1..MAX_MEMBERS {
            bus.join(Join::Existing(id)).unwrap();
        }
        assert_eq!(bus.join(Join::Existing(id)), Err((close::ROOM_FULL, "room is full")));
    }

    #[test]
    fn too_many_rooms_are_refused() {
        let mut bus = Bus::new();
        for i in 1..MAX_ROOMS {
            bus.join(Join::Create(format!("room {i}"))).unwrap();
        }
        assert_eq!(
            bus.join(Join::Create("one more".into())),
            Err((close::TOO_MANY_ROOMS, "too many rooms"))
        );
    }

    #[test]
    fn an_empty_room_disappears_but_the_open_room_stays() {
        let mut bus = Bus::new();
        let (a, _) = bus.join(Join::Create("temp".into())).unwrap();
        assert_eq!(bus.list().len(), 2);
        bus.leave(a);
        assert_eq!(bus.list().len(), 1, "the temp room is gone");
        assert_eq!(bus.list()[0].id, OPEN_ROOM);
        let (b, _) = bus.join(Join::Open).unwrap();
        bus.leave(b);
        assert_eq!(bus.list()[0].id, OPEN_ROOM);
        assert_eq!(bus.list()[0].owner, None);
    }

    #[test]
    fn the_lowest_member_owns_and_ownership_moves_on_leave() {
        let mut bus = Bus::new();
        let (a, _) = bus.join(Join::Create("x".into())).unwrap();
        let id = bus.list()[1].id;
        let (b, _) = bus.join(Join::Existing(id)).unwrap();
        // Only the owner may rename.
        assert!(bus.text(b, "title:renamed").is_empty());
        let out = bus.text(a, "title:renamed");
        assert!(texts(&out)[0].contains("renamed"));
        bus.leave(a);
        assert_eq!(bus.list().iter().find(|r| r.id == id).unwrap().owner, Some(b));
        // The new owner may kick now.
        let (c, _) = bus.join(Join::Existing(id)).unwrap();
        let out = bus.text(b, &format!("kick:{c}"));
        assert!(out.iter().any(|o| matches!(
            &o.frame,
            Frame::Close(close::KICKED, _) if o.to == c
        )));
    }

    #[test]
    fn the_owner_cannot_kick_itself() {
        let mut bus = Bus::new();
        let (a, _) = bus.join(Join::Open).unwrap();
        assert!(bus.text(a, &format!("kick:{a}")).is_empty());
    }

    #[test]
    fn binary_relay_prefixes_the_sender_and_reaches_everyone() {
        let mut bus = Bus::new();
        let (a, _) = bus.join(Join::Open).unwrap();
        let (b, _) = bus.join(Join::Open).unwrap();
        let out = bus.binary(a, b"hello");
        assert_eq!(out.len(), 2, "sender included: the bus echoes");
        for o in out {
            match o.frame {
                Frame::Binary(bytes) => {
                    assert_eq!(&bytes[..4], &a.to_le_bytes());
                    assert_eq!(&bytes[4..], b"hello");
                }
                _ => panic!("expected binary"),
            }
        }
        let out = bus.binary(b, &vec![0u8; MAX_MESSAGE + 1]);
        assert!(out.is_empty(), "a message over 4 KiB is dropped");
    }

    #[test]
    fn titles_are_trimmed_capped_and_defaulted() {
        assert_eq!(clean_title("  hi  "), Some("hi".into()));
        assert_eq!(clean_title("   "), None);
        let long = "x".repeat(40);
        assert_eq!(clean_title(&long).unwrap().chars().count(), 32);
        let mut bus = Bus::new();
        bus.join(Join::Create("   ".into())).unwrap();
        assert_eq!(bus.list()[1].title, "Room 1");
    }

    #[test]
    fn the_list_is_in_creation_order() {
        let mut bus = Bus::new();
        bus.join(Join::Create("b".into())).unwrap();
        bus.join(Join::Create("a".into())).unwrap();
        let listed = bus.list();
        let titles: Vec<_> = listed.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, ["Open room", "b", "a"]);
    }

    #[test]
    fn status_frames_are_valid_json() {
        let mut bus = Bus::new();
        let (_, outs) = bus.join(Join::Create(r#"a "quoted" title"#.into())).unwrap();
        for text in texts(&outs) {
            let body = text.strip_prefix("room:").unwrap();
            let parsed: serde_json::Value = serde_json::from_str(body).unwrap();
            assert_eq!(parsed["title"], "a \"quoted\" title");
        }
    }
}

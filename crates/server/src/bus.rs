//! The bus: rooms that pass binary messages between their members in one order.
//!
//! The bus takes any WebSocket, so a test drives it over memory with no sockets and no TLS.
//!
//! # Slow members
//!
//! The bus never waits for a member. Each member has a queue of [`QUEUE_FRAMES`] frames, and a
//! room puts each frame into the queue of every member while it holds the lock of the bus, which
//! gives all members one order. A member whose queue is full is removed from its room, and its
//! connection closes with the code 4004. A member whose socket accepts no frame for
//! [`WRITE_TIMEOUT`] loses its connection too. The other members see neither delay.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use sim::room::{
	CloseCode, Control, Join, MAX_MEMBERS, MAX_MESSAGE_BYTES, MAX_ROOMS, OPEN_ROOM_ID, OPEN_ROOM_TITLE, RoomEntry,
	RoomStatus, clean_info, clean_title, frame,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::protocol::frame::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode as WsCloseCode;

/// The frames that wait for one member. At 30 states a second from each of 16 players, this is
/// about two seconds of delay that a member can fall behind.
const QUEUE_FRAMES: usize = 1024;
/// How long a member may take to accept one frame.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a closing connection waits for the answer of the peer.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

/// Return the settings of every WebSocket of the bus.
///
/// The bus drops a message of more than [`MAX_MESSAGE_BYTES`], so it must read one first. It
/// reads up to 16 times that size, and a larger message ends the connection.
#[must_use]
pub fn socket_config() -> WebSocketConfig {
	WebSocketConfig::default()
		.max_message_size(Some(MAX_MESSAGE_BYTES * 16))
		.max_frame_size(Some(MAX_MESSAGE_BYTES * 16))
}

/// The rooms of one server. A clone shares the same rooms.
#[derive(Clone, Debug)]
pub struct Bus {
	state: Arc<Mutex<State>>,
}

impl Default for Bus {
	fn default() -> Self {
		Self::new()
	}
}

impl Bus {
	/// Return a bus that holds the open room and no other room.
	#[must_use]
	pub fn new() -> Self {
		let open = Room {
			id: OPEN_ROOM_ID.to_owned(),
			title: OPEN_ROOM_TITLE.to_owned(),
			info: String::new(),
			members: Vec::new(),
		};
		Self {
			state: Arc::new(Mutex::new(State {
				next_player: 1,
				next_room: 1,
				rooms: vec![open],
			})),
		}
	}

	/// Return the rooms in the order of creation.
	#[must_use]
	pub fn rooms(&self) -> Vec<RoomEntry> {
		self.lock().rooms.iter().map(Room::entry).collect()
	}

	/// Serve one member until its connection ends.
	///
	/// A refused join ends with a close frame of 4001, 4002, or 4003.
	pub async fn connect<S>(&self, mut ws: WebSocketStream<S>, join: Join)
	where
		S: AsyncRead + AsyncWrite + Unpin,
	{
		let Seat {
			room,
			number,
			mut queue,
			mut fate,
		} = match self.enter(join) {
			Ok(seat) => seat,
			Err(code) => {
				finish(&mut ws, close_frame(code)).await;
				return;
			}
		};
		loop {
			tokio::select! {
				incoming = ws.next() => match incoming {
					Some(Ok(Message::Binary(message))) => self.relay(&room, number, &message),
					Some(Ok(Message::Text(text))) => self.control(&room, number, &text),
					Some(Ok(Message::Close(_))) => {
						// The reply to the close frame waits in the socket until it is flushed.
						let _ = timeout(WRITE_TIMEOUT, ws.flush()).await;
						break;
					}
					Some(Err(_)) | None => break,
					Some(Ok(_)) => {}
				},
				outgoing = queue.recv() => match outgoing {
					Some(message) => {
						if !matches!(timeout(WRITE_TIMEOUT, ws.send(message)).await, Ok(Ok(()))) {
							break;
						}
					}
					None => {
						// The room sent the reason before it dropped the queue.
						let why = fate.try_recv().unwrap_or(CloseCode::FellBehind);
						finish(&mut ws, close_frame(why)).await;
						break;
					}
				},
			}
		}
		self.leave(&room, number);
	}

	fn lock(&self) -> MutexGuard<'_, State> {
		// A panic in another connection leaves the rooms whole, because each step keeps them valid.
		self.state.lock().unwrap_or_else(PoisonError::into_inner)
	}

	/// Run `act` on the room with `id`, and remove the rooms that it left empty if it says that
	/// a member left.
	fn with_room(&self, id: &str, act: impl FnOnce(&mut Room) -> bool) {
		let mut state = self.lock();
		if let Some(room) = state.rooms.iter_mut().find(|room| room.id == id)
			&& act(room)
		{
			state.tidy();
		}
	}

	/// Add a member to the room that `join` names, and tell the room.
	fn enter(&self, join: Join) -> Result<Seat, CloseCode> {
		let state = &mut *self.lock();
		let position = match join {
			Join::Open => state.rooms.iter().position(|room| room.id == OPEN_ROOM_ID),
			Join::Room(id) => state.rooms.iter().position(|room| room.id == id),
			Join::Create(title) => {
				if state.rooms.len() >= MAX_ROOMS {
					return Err(CloseCode::TooManyRooms);
				}
				let id = state.next_room.to_string();
				state.next_room += 1;
				state.rooms.push(Room {
					title: clean_title(&title, &id),
					id,
					info: String::new(),
					members: Vec::new(),
				});
				Some(state.rooms.len() - 1)
			}
		};
		let room = position
			.map(|position| &mut state.rooms[position])
			.ok_or(CloseCode::NoSuchRoom)?;
		if room.members.len() >= MAX_MEMBERS {
			return Err(CloseCode::RoomFull);
		}
		let number = state.next_player;
		state.next_player += 1;
		let (queue, receiver) = mpsc::channel(QUEUE_FRAMES);
		let (fate_sender, fate) = oneshot::channel();
		room.members.push(Member {
			number,
			queue,
			fate: fate_sender,
		});
		// A room that was just joined is not empty, so nothing to tidy even if the status removed a member.
		room.announce();
		Ok(Seat {
			room: room.id.clone(),
			number,
			queue: receiver,
			fate,
		})
	}

	/// Send a binary message of a member to the whole room, the sender included.
	fn relay(&self, room: &str, sender: u32, message: &[u8]) {
		if message.len() > MAX_MESSAGE_BYTES {
			return;
		}
		self.with_room(room, |room| {
			if !room.has(sender) {
				return false;
			}
			let framed = Bytes::from(frame(sender, message));
			let left = room.send_all(|_| Message::Binary(framed.clone()));
			if left {
				room.announce();
			}
			left
		});
	}

	/// Apply a request of a member, if it came from the owner.
	fn control(&self, room: &str, sender: u32, text: &str) {
		let Some(request) = Control::parse_frame(text) else {
			return;
		};
		self.with_room(room, |room| {
			if room.owner() != Some(sender) {
				return false;
			}
			match request {
				Control::Info(text) => {
					room.info = clean_info(&text);
					false
				}
				Control::Title(text) => {
					let title = clean_title(&text, &room.id);
					if title == room.title {
						return false;
					}
					room.title = title;
					room.announce()
				}
				Control::Kick(number) => {
					let Some(position) = room.members.iter().position(|member| member.number == number) else {
						return false;
					};
					room.members.remove(position).remove(CloseCode::Removed);
					room.announce();
					true
				}
			}
		});
	}

	/// Remove a member whose connection ended, and tell the room.
	fn leave(&self, room: &str, number: u32) {
		self.with_room(room, |room| {
			let present = room.has(number);
			if present {
				room.members.retain(|member| member.number != number);
				room.announce();
			}
			present
		});
	}
}

#[derive(Debug)]
struct State {
	/// The number of the next player. Numbers are unique on the whole server.
	next_player: u32,
	/// The id of the next room. Ids count up and no id returns.
	next_room: u32,
	/// The rooms in the order of creation.
	rooms: Vec<Room>,
}

impl State {
	/// Remove the rooms that no member is left in, except the open room.
	fn tidy(&mut self) {
		self.rooms
			.retain(|room| room.id == OPEN_ROOM_ID || !room.members.is_empty());
	}
}

#[derive(Debug)]
struct Room {
	id: String,
	title: String,
	info: String,
	/// The members in the order in which they joined.
	members: Vec<Member>,
}

#[derive(Debug)]
struct Member {
	number: u32,
	queue: mpsc::Sender<Message>,
	/// Where the room says why it removed the member. It cannot get lost behind a full queue.
	fate: oneshot::Sender<CloseCode>,
}

impl Member {
	/// Remove the member from its room for `why`, and close its queue.
	fn remove(self, why: CloseCode) {
		// The connection may have ended already, and then nobody needs the reason.
		let _ = self.fate.send(why);
	}
}

/// What a connection holds for a member that joined a room.
struct Seat {
	room: String,
	number: u32,
	/// The frames for the member. It ends when the room removed the member.
	queue: mpsc::Receiver<Message>,
	fate: oneshot::Receiver<CloseCode>,
}

impl Room {
	fn has(&self, number: u32) -> bool {
		self.members.iter().any(|member| member.number == number)
	}

	/// Return the number of the owner: the lowest number among the members.
	fn owner(&self) -> Option<u32> {
		self.members.iter().map(|member| member.number).min()
	}

	fn entry(&self) -> RoomEntry {
		RoomEntry {
			id: self.id.clone(),
			title: self.title.clone(),
			members: self.members.len(),
			owner: self.owner(),
			info: self.info.clone(),
		}
	}

	/// Put a message for each member into its queue, and remove the members whose queue is full.
	/// Return whether it removed a member.
	fn send_all(&mut self, message: impl Fn(u32) -> Message) -> bool {
		let before = self.members.len();
		self.members
			.retain(|member| member.queue.try_send(message(member.number)).is_ok());
		self.members.len() != before
	}

	/// Tell every member the state of the room. A member that cannot take it is removed, and the
	/// rest hear of that too. Return whether it removed a member.
	fn announce(&mut self) -> bool {
		let mut removed = false;
		loop {
			let status = RoomStatus {
				id: self.id.clone(),
				title: self.title.clone(),
				owner: self.owner().unwrap_or_default(),
				members: self.members.iter().map(|member| member.number).collect(),
				you: 0,
			};
			if !self.send_all(|you| Message::text(RoomStatus { you, ..status.clone() }.to_frame())) {
				return removed;
			}
			removed = true;
		}
	}
}

fn close_frame(code: CloseCode) -> Option<CloseFrame> {
	Some(CloseFrame {
		code: WsCloseCode::from(code.code()),
		reason: code.reason().into(),
	})
}

/// Send a close frame, and wait a moment for the answer, so that the peer reads the code.
async fn finish<S>(ws: &mut WebSocketStream<S>, closing: Option<CloseFrame>)
where
	S: AsyncRead + AsyncWrite + Unpin,
{
	if timeout(WRITE_TIMEOUT, ws.send(Message::Close(closing))).await.is_ok() {
		let _ = timeout(CLOSE_TIMEOUT, async { while let Some(Ok(_)) = ws.next().await {} }).await;
	}
}

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use futures_util::{SinkExt, StreamExt};
	use sim::room::{
		CloseCode, Control, Join, MAX_MEMBERS, MAX_MESSAGE_BYTES, MAX_ROOMS, OPEN_ROOM_ID, RoomStatus, split_frame,
	};
	use tokio::io::DuplexStream;
	use tokio::time::timeout;
	use tokio_tungstenite::WebSocketStream;
	use tokio_tungstenite::tungstenite::Message;
	use tokio_tungstenite::tungstenite::protocol::Role;

	use super::{Bus, QUEUE_FRAMES, socket_config};

	/// The client end of a member, with real WebSocket framing.
	struct Client {
		ws: WebSocketStream<DuplexStream>,
	}

	impl Client {
		async fn join(bus: &Bus, join: Join) -> Self {
			let (near, far) = tokio::io::duplex(1 << 16);
			let server = WebSocketStream::from_raw_socket(far, Role::Server, Some(socket_config())).await;
			let bus = bus.clone();
			tokio::spawn(async move { bus.connect(server, join).await });
			Self {
				ws: WebSocketStream::from_raw_socket(near, Role::Client, None).await,
			}
		}

		async fn next(&mut self) -> Message {
			self.next_within(Duration::from_secs(5)).await
		}

		/// Return the next frame that is not a ping or a pong. Reading also answers the pings.
		async fn next_within(&mut self, limit: Duration) -> Message {
			loop {
				let frame = timeout(limit, self.ws.next())
					.await
					.expect("the bus must send a frame in time")
					.expect("the connection must stay open")
					.expect("the frame must be valid");
				if !matches!(frame, Message::Ping(_) | Message::Pong(_)) {
					return frame;
				}
			}
		}

		async fn status(&mut self) -> RoomStatus {
			match self.next().await {
				Message::Text(text) => RoomStatus::parse_frame(&text).expect("the text must be a room status"),
				other => panic!("expected a room status, got {other:?}"),
			}
		}

		async fn send_binary(&mut self, message: &[u8]) {
			self.ws
				.send(Message::Binary(message.to_vec().into()))
				.await
				.expect("the bus must take the frame");
		}

		async fn send_text(&mut self, text: &str) {
			self.ws
				.send(Message::text(text))
				.await
				.expect("the bus must take the frame");
		}

		async fn binary(&mut self) -> (u32, Vec<u8>) {
			match self.next().await {
				Message::Binary(frame) => {
					let (sender, message) = split_frame(&frame).expect("a frame starts with a sender");
					(sender, message.to_vec())
				}
				other => panic!("expected a binary frame, got {other:?}"),
			}
		}

		/// Return the code of the close frame that ends the connection.
		async fn close_code(&mut self) -> u16 {
			loop {
				if let Message::Close(Some(closing)) = self.next().await {
					return closing.code.into();
				}
			}
		}
	}

	#[tokio::test]
	async fn a_player_without_a_query_joins_the_open_room() {
		let bus = Bus::new();
		let mut first = Client::join(&bus, Join::Open).await;
		let status = first.status().await;
		assert_eq!(status.id, OPEN_ROOM_ID, "the open room has its own id");
		assert_eq!(status.title, "Open room");
		assert_eq!(
			(status.owner, status.members, status.you),
			(1, vec![1], 1),
			"player numbers count up from 1"
		);
	}

	async fn room_of(bus: &Bus, title: &str) -> (Client, String) {
		let mut owner = Client::join(bus, Join::Create(title.into())).await;
		let id = owner.status().await.id;
		(owner, id)
	}

	#[tokio::test]
	async fn a_player_creates_a_room_and_another_joins_it_by_id() {
		let bus = Bus::new();
		let mut owner = Client::join(&bus, Join::Create("  Friday  ".into())).await;
		let status = owner.status().await;
		assert_eq!(
			(status.id.as_str(), status.title.as_str()),
			("1", "Friday"),
			"the title is trimmed"
		);
		let mut guest = Client::join(&bus, Join::Room("1".into())).await;
		let status = guest.status().await;
		assert_eq!((status.owner, status.members, status.you), (1, vec![1, 2], 2));
	}

	#[tokio::test]
	async fn player_numbers_are_unique_over_all_rooms() {
		let bus = Bus::new();
		let mut open = Client::join(&bus, Join::Open).await;
		let mut owner = Client::join(&bus, Join::Create("A".into())).await;
		assert_eq!(open.status().await.you, 1);
		assert_eq!(owner.status().await.you, 2, "the number counts on in another room");
	}

	#[tokio::test]
	async fn a_join_of_a_missing_room_is_refused_with_4001() {
		let bus = Bus::new();
		for id in ["9", "x", "00"] {
			let mut player = Client::join(&bus, Join::Room(id.into())).await;
			assert_eq!(player.close_code().await, 4001, "room {id:?} does not exist");
		}
	}

	#[tokio::test]
	async fn a_join_of_a_full_room_is_refused_with_4002() {
		let bus = Bus::new();
		let (owner, id) = room_of(&bus, "Full").await;
		let mut members = vec![owner];
		for _ in 1..MAX_MEMBERS {
			members.push(Client::join(&bus, Join::Room(id.clone())).await);
		}
		for member in &mut members {
			// The last status shows that every member is in.
			while member.status().await.members.len() < MAX_MEMBERS {}
		}
		let mut late = Client::join(&bus, Join::Room(id)).await;
		assert_eq!(late.close_code().await, 4002);
		assert_eq!(bus.rooms()[1].members, MAX_MEMBERS, "the refused player did not join");
	}

	#[tokio::test]
	async fn a_new_room_is_refused_with_4003_when_the_server_holds_64() {
		let bus = Bus::new();
		let mut owners = Vec::new();
		for number in 1..MAX_ROOMS {
			owners.push(room_of(&bus, &format!("Room {number}")).await);
		}
		assert_eq!(bus.rooms().len(), MAX_ROOMS, "the open room counts");
		let mut refused = Client::join(&bus, Join::Create("One more".into())).await;
		assert_eq!(refused.close_code().await, 4003);
		let mut open = Client::join(&bus, Join::Open).await;
		open.status().await;
	}

	#[tokio::test]
	async fn every_member_hears_of_a_join_a_leave_and_a_new_owner() {
		let bus = Bus::new();
		let (mut first, id) = room_of(&bus, "Hall").await;
		let mut second = Client::join(&bus, Join::Room(id.clone())).await;
		second.status().await;
		let status = first.status().await;
		assert_eq!(
			(status.members, status.owner),
			(vec![1, 2], 1),
			"the first member hears of the join"
		);
		let mut third = Client::join(&bus, Join::Room(id)).await;
		assert_eq!(third.status().await.members, vec![1, 2, 3]);
		assert_eq!(second.status().await.members, vec![1, 2, 3]);
		first.ws.close(None).await.expect("the close must go out");
		drop(first);
		let status = second.status().await;
		assert_eq!(
			(status.members, status.owner, status.you),
			(vec![2, 3], 2, 2),
			"the lowest number that is left owns the room"
		);
	}

	#[tokio::test]
	async fn a_binary_message_reaches_every_member_with_the_sender_in_front() {
		let bus = Bus::new();
		let mut first = Client::join(&bus, Join::Open).await;
		let mut second = Client::join(&bus, Join::Open).await;
		first.status().await;
		first.status().await;
		second.status().await;
		second.send_binary(b"hello").await;
		assert_eq!(first.binary().await, (2, b"hello".to_vec()));
		assert_eq!(
			second.binary().await,
			(2, b"hello".to_vec()),
			"the sender hears its own message"
		);
	}

	#[tokio::test]
	async fn all_members_see_the_messages_of_all_senders_in_one_order() {
		let bus = Bus::new();
		let mut clients = Vec::new();
		for _ in 0..4 {
			clients.push(Client::join(&bus, Join::Open).await);
		}
		for client in &mut clients {
			while client.status().await.members.len() < 4 {}
		}
		let (mut sinks, mut streams) = (Vec::new(), Vec::new());
		for client in clients {
			let (sink, stream) = client.ws.split();
			sinks.push(sink);
			streams.push(stream);
		}
		let senders: Vec<_> = sinks
			.into_iter()
			.enumerate()
			.map(|(index, mut sink)| {
				tokio::spawn(async move {
					for count in 0..100_u8 {
						let message = Message::Binary(vec![index as u8, count].into());
						sink.send(message).await.expect("the bus must take the frame");
					}
					sink
				})
			})
			.collect();
		let mut seen = Vec::new();
		for stream in &mut streams {
			let mut order = Vec::new();
			while order.len() < 400 {
				if let Message::Binary(frame) = stream.next().await.expect("open").expect("valid") {
					order.push(
						split_frame(&frame)
							.map(|(sender, message)| (sender, message.to_vec()))
							.expect("a sender"),
					);
				}
			}
			seen.push(order);
		}
		for order in &seen[1..] {
			assert_eq!(order, &seen[0], "every member must see the same order");
		}
		for sender in senders {
			let _sink = sender.await.expect("the sender task must finish");
		}
	}

	#[tokio::test]
	async fn a_message_over_the_limit_is_dropped_and_the_limit_itself_passes() {
		let bus = Bus::new();
		let mut player = Client::join(&bus, Join::Open).await;
		player.status().await;
		player.send_binary(&vec![7; MAX_MESSAGE_BYTES + 1]).await;
		player.send_binary(&vec![8; MAX_MESSAGE_BYTES]).await;
		let (sender, message) = player.binary().await;
		assert_eq!(
			(sender, message.len(), message[0]),
			(1, MAX_MESSAGE_BYTES, 8),
			"only the small one came back"
		);
	}

	#[tokio::test]
	async fn the_owner_sets_the_info_and_the_title_of_the_room() {
		let bus = Bus::new();
		let (mut owner, _) = room_of(&bus, "Old").await;
		owner.send_text("info:  3 bots: Ace ").await;
		owner.send_text("title:New").await;
		let status = owner.status().await;
		assert_eq!(status.title, "New", "a rename reaches the owner as a status");
		let rooms = bus.rooms();
		assert_eq!(
			(rooms[1].title.as_str(), rooms[1].info.as_str()),
			("New", "3 bots: Ace")
		);
	}

	#[tokio::test]
	async fn the_text_of_the_owner_is_never_relayed() {
		let bus = Bus::new();
		let mut owner = Client::join(&bus, Join::Open).await;
		let mut guest = Client::join(&bus, Join::Open).await;
		owner.status().await;
		owner.status().await;
		guest.status().await;
		owner.send_text("info:hello").await;
		owner.send_text("anything else").await;
		owner.send_binary(b"after").await;
		assert_eq!(
			guest.binary().await,
			(1, b"after".to_vec()),
			"no text came before the binary message"
		);
	}

	#[tokio::test]
	async fn the_owner_removes_a_member_with_the_code_4000() {
		let bus = Bus::new();
		let (mut owner, id) = room_of(&bus, "Kick").await;
		let mut guest = Client::join(&bus, Join::Room(id)).await;
		guest.status().await;
		owner.status().await;
		owner.send_text("kick:2").await;
		assert_eq!(guest.close_code().await, 4000);
		assert_eq!(
			owner.status().await.members,
			vec![1],
			"the others hear that the member is gone"
		);
	}

	#[tokio::test]
	async fn the_same_text_from_another_member_does_nothing() {
		let bus = Bus::new();
		let (mut owner, id) = room_of(&bus, "Mine").await;
		let mut guest = Client::join(&bus, Join::Room(id)).await;
		guest.status().await;
		owner.status().await;
		for text in ["info:theirs", "title:Theirs", "kick:1"] {
			guest.send_text(text).await;
		}
		guest.send_binary(b"barrier").await;
		assert_eq!(
			owner.binary().await,
			(2, b"barrier".to_vec()),
			"no status came before the message"
		);
		let rooms = bus.rooms();
		assert_eq!(
			(rooms[1].title.as_str(), rooms[1].info.as_str(), rooms[1].members),
			("Mine", "", 2)
		);
	}

	#[tokio::test]
	async fn an_empty_room_disappears_and_the_open_room_stays() {
		let bus = Bus::new();
		let (mut owner, _) = room_of(&bus, "Brief").await;
		let mut open = Client::join(&bus, Join::Open).await;
		open.status().await;
		assert_eq!(bus.rooms().len(), 2);
		owner.ws.close(None).await.expect("the close must go out");
		drop(owner);
		open.ws.close(None).await.expect("the close must go out");
		drop(open);
		timeout(Duration::from_secs(5), async {
			while bus.rooms().iter().any(|room| room.members > 0) {
				tokio::task::yield_now().await;
			}
		})
		.await
		.expect("both members must leave");
		let rooms = bus.rooms();
		assert_eq!(rooms.len(), 1, "only the open room is left");
		assert_eq!((rooms[0].id.as_str(), rooms[0].owner), ("0", None));
	}

	#[tokio::test]
	async fn the_room_list_gives_the_rooms_in_the_order_of_creation() {
		let bus = Bus::new();
		let mut open = Client::join(&bus, Join::Open).await;
		let (mut second, _) = room_of(&bus, "Second").await;
		let (_third, _) = room_of(&bus, "").await;
		open.status().await;
		second.send_text("info:Match").await;
		second.send_binary(b"barrier").await;
		second.binary().await;
		let list: Vec<_> = bus
			.rooms()
			.into_iter()
			.map(|room| (room.id, room.title, room.members, room.owner, room.info))
			.collect();
		let expected = [
			("0", "Open room", 1, Some(1), ""),
			("1", "Second", 1, Some(2), "Match"),
			("2", "Room 2", 1, Some(3), ""),
		]
		.map(|(id, title, members, owner, info)| (id.into(), title.into(), members, owner, info.into()));
		assert_eq!(list, expected);
	}

	#[tokio::test]
	async fn a_member_that_does_not_read_is_removed_and_the_others_go_on() {
		let bus = Bus::new();
		let (near, far) = tokio::io::duplex(1024);
		let server = WebSocketStream::from_raw_socket(far, Role::Server, None).await;
		let slow_bus = bus.clone();
		tokio::spawn(async move { slow_bus.connect(server, Join::Open).await });
		let mut slow = Client {
			ws: WebSocketStream::from_raw_socket(near, Role::Client, None).await,
		};
		let fast = Client::join(&bus, Join::Open).await;
		let (mut sink, mut stream) = fast.ws.split();
		let sender = tokio::spawn(async move {
			for count in 0..4000_u16 {
				sink.send(Message::Binary(count.to_le_bytes().to_vec().into()))
					.await
					.expect("the bus must take it");
			}
		});
		let mut received = 0;
		while received < 4000 {
			if let Message::Binary(_) = stream.next().await.expect("open").expect("valid") {
				received += 1;
			}
		}
		sender.await.expect("the sender task must finish");
		assert_eq!(bus.rooms()[0].members, 1, "the member that fell behind was removed");
		assert_eq!(slow.close_code().await, 4004, "it learns why, when it reads again");
	}

	#[tokio::test]
	async fn a_removed_member_with_a_full_queue_still_learns_why() {
		let bus = Bus::new();
		let mut owner = bus.enter(Join::Open).expect("the owner joins");
		let mut victim = bus.enter(Join::Open).expect("the victim joins");
		// The victim never reads. Fill its queue to the last place, but not past it.
		for _ in victim.queue.len()..QUEUE_FRAMES {
			bus.relay(&owner.room, owner.number, b"x");
			while owner.queue.try_recv().is_ok() {}
		}
		assert_eq!(bus.rooms()[0].members, 2, "a full queue is not yet too far behind");
		bus.control(&owner.room, owner.number, &Control::Kick(victim.number).to_frame());
		assert_eq!(bus.rooms()[0].members, 1, "the owner removed the victim");
		while victim.queue.recv().await.is_some() {}
		assert_eq!(
			victim.fate.try_recv(),
			Ok(CloseCode::Removed),
			"the reason is not lost behind the queue"
		);
	}

	#[tokio::test]
	async fn a_member_that_closes_its_socket_gets_the_close_reply_and_no_reset() {
		let bus = Bus::new();
		let mut player = Client::join(&bus, Join::Open).await;
		player.status().await;
		player.ws.close(None).await.expect("the close must go out");
		let mut replies = 0;
		timeout(Duration::from_secs(5), async {
			while let Some(frame) = player.ws.next().await {
				let frame = frame.expect("the stream must end with the close handshake, not with a reset");
				replies += usize::from(matches!(frame, Message::Close(_)));
			}
		})
		.await
		.expect("the stream must end");
		assert_eq!(replies, 1, "the bus must answer the close frame with its own");
	}
}

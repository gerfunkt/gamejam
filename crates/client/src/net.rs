//! The connection to the message bus, and the room list of the server.
//!
//! The game sends through two calls, a state and an event. Both use one WebSocket today, and
//! another transport can take their place behind these calls.

use std::cell::RefCell;

use sim::{
	room::{Control, Join, RoomEntry, RoomStatus, parse_room_list, split_frame},
	wire::Message,
};
use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::JsFuture;
use web_sys::{BinaryType, CloseEvent, MessageEvent, WebSocket};

use crate::page;

/// The name under which the page stores the address of the game server that the player chose.
const SERVER_KEY: &str = "server";

/// Return the address of the game server that the player chose, such as `192.168.1.20:5201`.
///
/// Without one, the client talks to the server that served the page. A copy of the game on a host of
/// static files, such as GitHub Pages, has no server of its own, so its players name one. The address
/// takes `?server=` from the page address first.
pub fn server() -> Option<String> {
	page::query("server")
		.or_else(|| page::load(SERVER_KEY))
		.map(|address| plain_address(&address))
		.filter(|address| !address.is_empty())
}

/// Remember the game server that the player chose. An empty address means the server of the page.
pub fn set_server(address: &str) {
	page::store(SERVER_KEY, &plain_address(address));
}

/// Return the host and the port of an address that a person typed, without its scheme and its path.
fn plain_address(address: &str) -> String {
	let address = address.trim();
	let address = ["https://", "http://", "wss://", "ws://"]
		.iter()
		.find_map(|scheme| address.strip_prefix(scheme))
		.unwrap_or(address);
	address.split('/').next().unwrap_or_default().to_owned()
}

/// Something that arrived from the bus.
pub enum Incoming {
	/// The status of the room.
	Status(RoomStatus),
	/// A message of a member, with the time of its arrival on the clock of the page.
	Message { sender: u32, message: Message, at: f64 },
	/// The connection ended, with the code of the close frame.
	Closed { code: u16 },
}

#[derive(Default)]
struct Link {
	socket: Option<WebSocket>,
	/// The number of the current connection. A late event of an older connection is dropped.
	connection: u32,
	inbox: Vec<Incoming>,
	rooms: Option<Result<Vec<RoomEntry>, String>>,
}

thread_local! {
	static LINK: RefCell<Link> = RefCell::new(Link::default());
}

/// Open a connection to the bus of the game server, and enter a room.
///
/// The server is the one that the player chose, or else the one that served the page.
pub fn connect(join: &Join) {
	disconnect();
	let location = page::window().location();
	let address = match server() {
		// The game server always speaks HTTPS, and a page on a public site may only reach it so.
		Some(host) => format!("wss://{host}/ws{}", join.to_query()),
		None => {
			let scheme = if location.protocol().is_ok_and(|protocol| protocol == "https:") {
				"wss"
			} else {
				"ws"
			};
			format!(
				"{scheme}://{}/ws{}",
				location.host().unwrap_or_default(),
				join.to_query()
			)
		}
	};
	let Ok(socket) = WebSocket::new(&address) else {
		LINK.with_borrow_mut(|link| link.inbox.push(Incoming::Closed { code: 1006 }));
		return;
	};
	socket.set_binary_type(BinaryType::Arraybuffer);
	let connection = LINK.with_borrow(|link| link.connection);

	let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
		let at = page::now();
		let incoming = if let Ok(buffer) = event.data().dyn_into::<js_sys::ArrayBuffer>() {
			let frame = js_sys::Uint8Array::new(&buffer).to_vec();
			split_frame(&frame).and_then(|(sender, bytes)| {
				Some(Incoming::Message {
					sender,
					message: Message::decode(bytes)?,
					at,
				})
			})
		} else {
			event
				.data()
				.as_string()
				.and_then(|text| RoomStatus::parse_frame(&text))
				.map(Incoming::Status)
		};
		LINK.with_borrow_mut(|link| {
			if link.connection == connection {
				link.inbox.extend(incoming);
			}
		});
	});
	socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
	on_message.forget();

	let on_close = Closure::<dyn FnMut(CloseEvent)>::new(move |event: CloseEvent| {
		LINK.with_borrow_mut(|link| {
			if link.connection == connection {
				link.socket = None;
				link.inbox.push(Incoming::Closed { code: event.code() });
			}
		});
	});
	socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));
	on_close.forget();

	LINK.with_borrow_mut(|link| link.socket = Some(socket));
}

/// Close the connection, if one is open. Its late events no longer arrive.
pub fn disconnect() {
	LINK.with_borrow_mut(|link| {
		link.connection += 1;
		link.inbox.clear();
		if let Some(socket) = link.socket.take() {
			let _ = socket.close();
		}
	});
}

fn send(message: &Message) {
	LINK.with_borrow(|link| {
		if let Some(socket) = link
			.socket
			.as_ref()
			.filter(|socket| socket.ready_state() == WebSocket::OPEN)
		{
			let _ = socket.send_with_u8_array(&message.encode());
		}
	});
}

/// Send the state of a plane. A newer state replaces it, so the transport may lose it.
pub fn send_state(message: &Message) {
	send(message);
}

/// Send an event of the match or the lobby. The transport must deliver it, in order.
pub fn send_event(message: &Message) {
	send(message);
}

/// Send a request of the host to the bus.
pub fn send_control(control: &Control) {
	LINK.with_borrow(|link| {
		if let Some(socket) = link
			.socket
			.as_ref()
			.filter(|socket| socket.ready_state() == WebSocket::OPEN)
		{
			let _ = socket.send_with_str(&control.to_frame());
		}
	});
}

/// Take what arrived from the bus since the last call.
pub fn take_incoming() -> Vec<Incoming> {
	LINK.with_borrow_mut(|link| std::mem::take(&mut link.inbox))
}

/// Ask the server for its room list. The answer waits in [`take_rooms`].
pub fn request_rooms() {
	wasm_bindgen_futures::spawn_local(async {
		let rooms = fetch_rooms().await;
		LINK.with_borrow_mut(|link| link.rooms = Some(rooms));
	});
}

async fn fetch_rooms() -> Result<Vec<RoomEntry>, String> {
	let chosen = server();
	let reason = || match &chosen {
		Some(host) => format!(
			"The server at {host} gave no room list. Open https://{host} once in this browser, accept the \
			 warning about its certificate, come back, and refresh the list."
		),
		None => "This page has no game server. Run “cargo xtask serve” on a computer of your network, and \
			 open the address that it prints once in this browser to accept its certificate. Then enter \
			 that address under Game Server, such as 192.168.1.20:5201. Solo flight needs no server."
			.to_owned(),
	};
	let failed = |_| reason();
	let address = chosen
		.as_ref()
		.map_or_else(|| "/rooms".to_owned(), |host| format!("https://{host}/rooms"));
	let response = JsFuture::from(page::window().fetch_with_str(&address))
		.await
		.map_err(failed)?;
	let response: web_sys::Response = response.dyn_into().map_err(failed)?;
	let text = JsFuture::from(response.text().map_err(failed)?).await.map_err(failed)?;
	parse_room_list(&text.as_string().unwrap_or_default()).map_err(|_| reason())
}

/// Take the room list, if a new one arrived since the last call.
pub fn take_rooms() -> Option<Result<Vec<RoomEntry>, String>> {
	LINK.with_borrow_mut(|link| link.rooms.take())
}

use crate::hooks;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

#[derive(Debug)]
pub enum NetEvent {
    Open,
    Binary(Vec<u8>),
    Text(String),
    Close(u16, String),
    Error(String),
    Rooms(String),
}

type Inbox = Rc<RefCell<VecDeque<NetEvent>>>;

pub struct Net {
    socket: Option<web_sys::WebSocket>,
    inbox: Inbox,
    _open: Option<Closure<dyn FnMut(JsValue)>>,
    _message: Option<Closure<dyn FnMut(web_sys::MessageEvent)>>,
    _close: Option<Closure<dyn FnMut(web_sys::CloseEvent)>>,
    _error: Option<Closure<dyn FnMut(JsValue)>>,
}

impl Default for Net {
    fn default() -> Self {
        Self {
            socket: None,
            inbox: Rc::new(RefCell::new(VecDeque::new())),
            _open: None,
            _message: None,
            _close: None,
            _error: None,
        }
    }
}

impl Net {
    pub fn connect(&mut self, room: Option<&str>, create: Option<&str>) {
        let Some(window) = web_sys::window() else {
            return;
        };
        let Ok(protocol) = window.location().protocol() else {
            return;
        };
        let Ok(host) = window.location().host() else {
            return;
        };
        let scheme = if protocol == "https:" { "wss" } else { "ws" };
        let mut url = format!("{scheme}://{host}/ws");
        if let Some(room) = room {
            let room = js_sys::encode_uri_component(room);
            url.push_str(&format!("?room={room}"));
        } else if let Some(create) = create {
            let create = js_sys::encode_uri_component(create);
            url.push_str(&format!("?create={create}"));
        }
        let socket = match web_sys::WebSocket::new(&url) {
            Ok(socket) => socket,
            Err(e) => {
                self.inbox.borrow_mut().push_back(NetEvent::Error(format!("{e:?}")));
                return;
            }
        };
        socket.set_binary_type(web_sys::BinaryType::Arraybuffer);

        let inbox = self.inbox.clone();
        let open = Closure::wrap(Box::new(move |_: JsValue| {
            inbox.borrow_mut().push_back(NetEvent::Open);
        }) as Box<dyn FnMut(JsValue)>);
        socket.set_onopen(Some(open.as_ref().unchecked_ref()));

        let inbox = self.inbox.clone();
        let message = Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
            let data = event.data();
            if let Some(text) = data.as_string() {
                inbox.borrow_mut().push_back(NetEvent::Text(text));
            } else if let Ok(buffer) = data.dyn_into::<js_sys::ArrayBuffer>() {
                let array = js_sys::Uint8Array::new(&buffer);
                let mut bytes = vec![0; array.length() as usize];
                array.copy_to(&mut bytes);
                inbox.borrow_mut().push_back(NetEvent::Binary(bytes));
            }
        }) as Box<dyn FnMut(web_sys::MessageEvent)>);
        socket.set_onmessage(Some(message.as_ref().unchecked_ref()));

        let inbox = self.inbox.clone();
        let close = Closure::wrap(Box::new(move |event: web_sys::CloseEvent| {
            inbox
                .borrow_mut()
                .push_back(NetEvent::Close(event.code(), event.reason()));
        }) as Box<dyn FnMut(web_sys::CloseEvent)>);
        socket.set_onclose(Some(close.as_ref().unchecked_ref()));

        let inbox = self.inbox.clone();
        let error = Closure::wrap(Box::new(move |event: JsValue| {
            inbox.borrow_mut().push_back(NetEvent::Error(format!("{event:?}")));
        }) as Box<dyn FnMut(JsValue)>);
        socket.set_onerror(Some(error.as_ref().unchecked_ref()));

        self.socket = Some(socket);
        self._open = Some(open);
        self._message = Some(message);
        self._close = Some(close);
        self._error = Some(error);
    }

    pub fn connected(&self) -> bool {
        self.socket
            .as_ref()
            .is_some_and(|s| s.ready_state() == web_sys::WebSocket::OPEN)
    }

    pub fn send_binary(&self, bytes: &[u8]) {
        if let Some(socket) = &self.socket {
            let _ = socket.send_with_u8_array(bytes);
        }
    }

    pub fn send_text(&self, text: &str) {
        if let Some(socket) = &self.socket {
            let _ = socket.send_with_str(text);
        }
    }

    pub fn close(&mut self) {
        if let Some(socket) = self.socket.take() {
            let _ = socket.close();
        }
    }

    pub fn drain(&mut self) -> Vec<NetEvent> {
        self.inbox.borrow_mut().drain(..).collect()
    }

    pub fn fetch_rooms(&mut self) {
        let inbox = self.inbox.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let Some(window) = web_sys::window() else {
                return;
            };
            let promise = window.fetch_with_str("/rooms");
            let result = async {
                let value = wasm_bindgen_futures::JsFuture::from(promise).await.ok()?;
                let response: web_sys::Response = value.dyn_into().ok()?;
                let text = wasm_bindgen_futures::JsFuture::from(response.text().ok()?).await.ok()?;
                text.as_string()
            }
            .await;
            inbox.borrow_mut().push_back(match result {
                Some(body) => NetEvent::Rooms(body),
                None => NetEvent::Error("could not load the rooms".into()),
            });
        });
    }
}

pub fn send_game(net: &Net, message: &sim::wire::Message) {
    net.send_binary(&sim::wire::encode(message));
}

pub fn send_profile(net: &Net, id: u32, name: &str) {
    send_game(net, &sim::wire::Message::Profile { id, name: name.into() });
}

pub fn room_info(net: &Net, text: &str) {
    net.send_text(&sim::room::info(text));
}

pub fn room_title(net: &Net, text: &str) {
    net.send_text(&sim::room::title(text));
}

pub fn kick(net: &Net, num: u32) {
    net.send_text(&sim::room::kick(num));
}

pub fn current_clock() -> f64 {
    hooks::now_seconds()
}

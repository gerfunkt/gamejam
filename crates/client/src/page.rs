//! The page around the canvas: its elements, its events, and its storage.
//!
//! The page is HTML, and this module is the only code that touches it. A browser runs the
//! client on one thread, so the state of the page lives in thread-local cells.

use std::{cell::RefCell, collections::HashMap};

use wasm_bindgen::{JsCast, closure::Closure, convert::FromWasmAbi};
use web_sys::{Document, Element, Event, EventTarget, HtmlElement, HtmlInputElement, HtmlSelectElement, Window};

/// Something that the player did in a menu of the page.
#[derive(Clone, Debug, PartialEq)]
pub enum UiEvent {
	/// The player chose the control with this `data-action`. `value` is its `data-value`.
	Action { action: String, value: String },
	/// The player changed the field with this id to this value.
	Change { id: String, value: String },
}

thread_local! {
	static EVENTS: RefCell<Vec<UiEvent>> = const { RefCell::new(Vec::new()) };
	/// The last text and attributes that this module wrote, so that an unchanged value costs no layout.
	static WRITTEN: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// Return the window of the page.
pub fn window() -> Window {
	web_sys::window().expect("the client runs in a page")
}

/// Return the document of the page.
pub fn document() -> Document {
	window().document().expect("a page has a document")
}

/// Return the time since the page loaded, in milliseconds.
pub fn now() -> f64 {
	window().performance().expect("a page has a clock").now()
}

/// Return the element with this id. The page and the client are built together, so it exists.
pub fn element(id: &str) -> HtmlElement {
	let element = document()
		.get_element_by_id(id)
		.unwrap_or_else(|| panic!("the page has no element #{id}"));
	element.unchecked_into()
}

/// Call `handler` for each event of this kind on `target`, for as long as the page lives.
pub fn listen<E: FromWasmAbi + 'static>(target: &EventTarget, kind: &str, handler: impl FnMut(E) + 'static) {
	let closure = Closure::<dyn FnMut(E)>::new(handler);
	target
		.add_event_listener_with_callback(kind, closure.as_ref().unchecked_ref())
		.expect("the kind of an event is a valid name");
	closure.forget();
}

/// Call `handler` like [`listen`], with the right to stop the default action of the browser.
pub fn listen_active<E: FromWasmAbi + 'static>(target: &EventTarget, kind: &str, handler: impl FnMut(E) + 'static) {
	let closure = Closure::<dyn FnMut(E)>::new(handler);
	let options = web_sys::AddEventListenerOptions::new();
	options.set_passive(false);
	target
		.add_event_listener_with_callback_and_add_event_listener_options(
			kind,
			closure.as_ref().unchecked_ref(),
			&options,
		)
		.expect("the kind of an event is a valid name");
	closure.forget();
}

/// Start to collect what the player does in the menus.
pub fn watch_menus() {
	let ui: EventTarget = element("ui").into();
	listen(&ui, "click", |event: Event| {
		let chosen = event
			.target()
			.and_then(|target| target.dyn_into::<Element>().ok())
			.and_then(|target| target.closest("button[data-action]").ok().flatten());
		if let Some(button) = chosen {
			push_action(&button);
		}
	});
	listen(&ui, "submit", |event: Event| {
		event.prevent_default();
		if let Some(form) = event.target().and_then(|target| target.dyn_into::<Element>().ok()) {
			push_action(&form);
		}
	});
	listen(&ui, "input", |event: Event| {
		let Some(target) = event.target() else { return };
		let (id, value) = if let Some(input) = target.dyn_ref::<HtmlInputElement>() {
			let value = if input.type_() == "checkbox" {
				input.checked().to_string()
			} else {
				input.value()
			};
			(input.id(), value)
		} else if let Some(select) = target.dyn_ref::<HtmlSelectElement>() {
			(select.id(), select.value())
		} else {
			return;
		};
		EVENTS.with_borrow_mut(|events| events.push(UiEvent::Change { id, value }));
	});
}

fn push_action(control: &Element) {
	let action = control.get_attribute("data-action").unwrap_or_default();
	let value = control.get_attribute("data-value").unwrap_or_default();
	EVENTS.with_borrow_mut(|events| events.push(UiEvent::Action { action, value }));
}

/// Take what the player did in the menus since the last call.
pub fn take_ui_events() -> Vec<UiEvent> {
	EVENTS.with_borrow_mut(std::mem::take)
}

/// Return whether a write of `value` under `key` changes what the page shows, and remember it.
fn changed(key: String, value: &str) -> bool {
	WRITTEN.with_borrow_mut(|written| match written.get(&key) {
		Some(old) if old == value => false,
		_ => {
			written.insert(key, value.to_owned());
			true
		}
	})
}

/// Show `text` in the element with this id.
pub fn set_text(id: &str, text: &str) {
	if changed(format!("text {id}"), text) {
		element(id).set_text_content(Some(text));
	}
}

/// Hide the element with this id, or show it.
pub fn set_hidden(id: &str, hidden: bool) {
	if changed(format!("hidden {id}"), if hidden { "1" } else { "0" }) {
		element(id).set_hidden(hidden);
	}
}

/// Give the element with this id the class `class`, or take it away.
pub fn set_class(id: &str, class: &str, on: bool) {
	if changed(format!("class {id} {class}"), if on { "1" } else { "0" }) {
		let _ = element(id).class_list().toggle_with_force(class, on);
	}
}

/// Set one property of the style of the element with this id.
pub fn set_style(id: &str, property: &str, value: &str) {
	if changed(format!("style {id} {property}"), value) {
		let _ = element(id).style().set_property(property, value);
	}
}

/// Return the value of the text field with this id.
pub fn field(id: &str) -> String {
	element(id).unchecked_into::<HtmlInputElement>().value()
}

/// Put `value` into the field or the choice with this id, if it holds another value.
pub fn set_field(id: &str, value: &str) {
	let element = element(id);
	if let Some(select) = element.dyn_ref::<HtmlSelectElement>() {
		if select.value() != value {
			select.set_value(value);
		}
	} else if let Some(input) = element.dyn_ref::<HtmlInputElement>() {
		if input.type_() == "checkbox" {
			input.set_checked(value == "true");
		} else if input.value() != value {
			input.set_value(value);
		}
	}
}

/// Enable or disable the control with this id.
pub fn set_disabled(id: &str, disabled: bool) {
	if changed(format!("disabled {id}"), if disabled { "1" } else { "0" }) {
		let element = element(id);
		let _ = if disabled {
			element.set_attribute("disabled", "")
		} else {
			element.remove_attribute("disabled")
		};
	}
}

/// One element of a list or a table that this module builds: a tag, a text, and attributes.
pub struct Piece<'a> {
	pub tag: &'a str,
	pub text: &'a str,
	pub attributes: &'a [(&'a str, &'a str)],
}

/// Fill the element with this id with one row for each entry of `rows`.
///
/// Each row is an element with the tag `row_tag` and the class `class`, and it holds the pieces
/// of its entry. Texts enter the page as text, never as markup, because names come from other
/// players. The page changes only when the rows differ from the last call.
pub fn set_rows(id: &str, row_tag: &str, rows: &[(&str, Vec<Piece>)]) {
	let mut key = String::new();
	for (class, pieces) in rows {
		key.push_str(class);
		for piece in pieces {
			key.push_str(&format!(
				"\u{1}{}\u{2}{}\u{2}{:?}",
				piece.tag, piece.text, piece.attributes
			));
		}
		key.push('\u{3}');
	}
	if !changed(format!("rows {id}"), &key) {
		return;
	}
	let (parent, document) = (element(id), document());
	parent.set_text_content(None);
	for (class, pieces) in rows {
		let row = document.create_element(row_tag).expect("a row tag is a valid name");
		if !class.is_empty() {
			row.set_class_name(class);
		}
		for piece in pieces {
			let child = document.create_element(piece.tag).expect("a piece tag is a valid name");
			child.set_text_content(Some(piece.text));
			for (name, value) in piece.attributes {
				let _ = child.set_attribute(name, value);
			}
			let _ = row.append_child(&child);
		}
		let _ = parent.append_child(&row);
	}
}

/// Return the value of a parameter in the address of the page.
pub fn query(name: &str) -> Option<String> {
	let search = window().location().search().ok()?;
	web_sys::UrlSearchParams::new_with_str(&search).ok()?.get(name)
}

/// Return the graphics interface for which the page loaded this client: "webgpu" or "webgl2".
pub fn backend() -> String {
	document()
		.document_element()
		.and_then(|root| root.get_attribute("data-backend"))
		.unwrap_or_default()
}

/// Read a value that an earlier visit stored. A browser can refuse storage, and it can delete it.
pub fn load(key: &str) -> Option<String> {
	window().local_storage().ok()??.get_item(key).ok()?
}

/// Store a value for later visits. A browser that refuses storage loses it, and nothing else.
pub fn store(key: &str, value: &str) {
	if let Ok(Some(storage)) = window().local_storage() {
		let _ = storage.set_item(key, value);
	}
}

/// Write a line to the console of the browser. The browser tests read these lines.
pub fn log(line: &str) {
	web_sys::console::log_1(&line.into());
}

/// Show a panic on the page, and its details in the console.
///
/// The engine panics when the browser gives it no graphics at all, before it can say so itself.
pub fn report_panics() {
	std::panic::set_hook(Box::new(|info| {
		console_error_panic_hook::hook(info);
		if let Some(status) = web_sys::window()
			.and_then(|window| window.document())
			.and_then(|document| document.get_element_by_id("status"))
		{
			status.set_text_content(Some(
				"The game stopped. This browser may give it no graphics: open the page in a current Chromium browser, over HTTPS. The console of the browser holds the details.",
			));
		}
	}));
}

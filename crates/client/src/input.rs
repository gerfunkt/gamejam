//! The input of the player: keys, the pointer, and the wheel, as actions of the game.
//!
//! The page delivers the events. The rules see only [`Controls`]. Each key can change, and the
//! choices of the player survive a visit.

use std::{cell::RefCell, collections::HashSet};

use bevy::math::{Quat, Vec2};
use serde::{Deserialize, Serialize};
use sim::{Controls, Skill, aim_direction};
use wasm_bindgen::JsCast;
use web_sys::{Element, Event, EventTarget, KeyboardEvent, PointerEvent, WheelEvent};

use crate::page;

/// How far a turn key or a nose key moves the aim, as the tangent of the angle.
const KEY_AIM: f32 = 0.8;
/// How much one notch of the wheel changes the throttle.
const WHEEL_THROTTLE: f32 = 0.1;
/// How much one second of a throttle key changes the throttle.
const KEY_THROTTLE: f32 = 0.7;
const STORAGE_KEY: &str = "dogfight-options";
/// The version of the stored options. Options of another version give way to the defaults.
const OPTIONS_VERSION: u32 = 1;

/// An action of the game that a key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bind {
	Fire,
	Launch,
	Boost,
	ThrottleUp,
	ThrottleDown,
	TurnLeft,
	TurnRight,
	NoseDown,
	NoseUp,
	Pause,
}

impl Bind {
	pub const ALL: [Bind; 10] = [
		Bind::Fire,
		Bind::Launch,
		Bind::Boost,
		Bind::ThrottleUp,
		Bind::ThrottleDown,
		Bind::TurnLeft,
		Bind::TurnRight,
		Bind::NoseDown,
		Bind::NoseUp,
		Bind::Pause,
	];

	/// The name of the action in the settings.
	pub fn label(self) -> &'static str {
		match self {
			Bind::Fire => "Fire the Gun",
			Bind::Launch => "Launch a Missile",
			Bind::Boost => "Boost",
			Bind::ThrottleUp => "More Throttle",
			Bind::ThrottleDown => "Less Throttle",
			Bind::TurnLeft => "Turn Left",
			Bind::TurnRight => "Turn Right",
			Bind::NoseDown => "Nose Down",
			Bind::NoseUp => "Nose Up",
			Bind::Pause => "Pause Menu",
		}
	}

	/// The key of the action on a new visit, as the `code` of a keyboard event. A code names
	/// the place of a key, so the keys stay together on every keyboard layout.
	fn default_key(self) -> &'static str {
		match self {
			Bind::Fire => "Space",
			Bind::Launch => "KeyF",
			Bind::Boost => "ShiftLeft",
			Bind::ThrottleUp => "KeyW",
			Bind::ThrottleDown => "KeyS",
			Bind::TurnLeft => "KeyA",
			Bind::TurnRight => "KeyD",
			Bind::NoseDown => "ArrowUp",
			Bind::NoseUp => "ArrowDown",
			Bind::Pause => "Escape",
		}
	}

	/// Return the action at this place of [`Bind::ALL`], as the page names it.
	pub fn from_index(index: &str) -> Option<Bind> {
		Bind::ALL.get(index.parse::<usize>().ok()?).copied()
	}
}

/// The choices that the player keeps between visits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Options {
	version: u32,
	/// The name of the player.
	pub name: String,
	/// How far the aim moves for a move of the mouse. 1 puts the aim under the cursor.
	pub sensitivity: f32,
	/// Whether a mouse above the centre lowers the nose.
	pub invert: bool,
	/// The key of each action, in the order of [`Bind::ALL`].
	keys: Vec<String>,
	/// The number of bots that the player chose last as a host.
	pub bots: u8,
	/// The skill of the bots that the player chose last as a host.
	pub skill: Skill,
}

impl Default for Options {
	fn default() -> Self {
		Self {
			version: OPTIONS_VERSION,
			name: String::new(),
			sensitivity: 1.0,
			invert: false,
			keys: Bind::ALL.iter().map(|bind| bind.default_key().to_owned()).collect(),
			bots: 4,
			skill: Skill::Pilot,
		}
	}
}

impl Options {
	/// Return the stored options, or the defaults when the store holds none that this version reads.
	pub fn load() -> Self {
		page::load(STORAGE_KEY)
			.and_then(|text| serde_json::from_str::<Options>(&text).ok())
			.filter(|options| options.version == OPTIONS_VERSION && options.keys.len() == Bind::ALL.len())
			.unwrap_or_default()
	}

	/// Store the options for the next visit.
	pub fn save(&self) {
		if let Ok(text) = serde_json::to_string(self) {
			page::store(STORAGE_KEY, &text);
		}
	}

	/// Return the key of an action, as the `code` of a keyboard event.
	pub fn key(&self, bind: Bind) -> &str {
		&self.keys[bind as usize]
	}

	/// Give an action this key. Another action that had the key gets the old key of this one.
	pub fn set_key(&mut self, bind: Bind, code: &str) {
		let old = self.keys[bind as usize].clone();
		if let Some(other) = self.keys.iter_mut().find(|key| *key == code) {
			*other = old;
		}
		self.keys[bind as usize] = code.to_owned();
	}

	/// Give every action its default key again.
	pub fn reset_keys(&mut self) {
		self.keys = Options::default().keys;
	}
}

/// Return a readable name for the `code` of a key, such as "W" for "KeyW".
pub fn key_name(code: &str) -> String {
	let spaced = |text: &str| {
		let mut name = String::new();
		for letter in text.chars() {
			if letter.is_uppercase() && !name.is_empty() {
				name.push(' ');
			}
			name.push(letter);
		}
		name
	};
	match code {
		_ if code.starts_with("Key") => code[3..].to_owned(),
		_ if code.starts_with("Digit") => code[5..].to_owned(),
		_ if code.starts_with("Arrow") => format!("{} Arrow", &code[5..]),
		_ => spaced(code),
	}
}

#[derive(Default)]
struct State {
	/// The codes of the keys that are down.
	down: HashSet<String>,
	/// The codes of the keys that went down since the last look.
	pressed: Vec<String>,
	/// The place of the pointer in the page, in CSS pixels.
	pointer: Option<Vec2>,
	left: bool,
	right: bool,
	/// The notches that the wheel turned since the last look. Up is positive.
	wheel: f32,
	/// Whether the page lost the focus or went out of view since the last look.
	left_page: bool,
	/// Whether a match takes the keys now, so that the browser must not act on them.
	in_flight: bool,
}

thread_local! {
	static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Start to collect the input of the player from the page.
pub fn watch() {
	let window: EventTarget = page::window().into();
	let canvas: EventTarget = page::element("game").into();
	page::listen_active(&window, "keydown", |event: KeyboardEvent| {
		let typing = event
			.target()
			.and_then(|target| target.dyn_into::<Element>().ok())
			.is_some_and(|target| matches!(target.tag_name().as_str(), "INPUT" | "SELECT" | "TEXTAREA"));
		STATE.with_borrow_mut(|state| {
			let browser_key = event.ctrl_key()
				|| event.meta_key()
				|| event.alt_key()
				|| event.code().starts_with('F') && event.code().len() <= 3;
			if state.in_flight && !typing && !browser_key {
				event.prevent_default();
			}
			if !typing && !event.repeat() {
				state.down.insert(event.code());
				state.pressed.push(event.code());
			}
		});
	});
	page::listen(&window, "keyup", |event: KeyboardEvent| {
		STATE.with_borrow_mut(|state| state.down.remove(&event.code()));
	});
	page::listen(&window, "blur", |_: Event| leave_page());
	page::listen(&page::document().into(), "visibilitychange", |_: Event| {
		if page::document().visibility_state() == web_sys::VisibilityState::Hidden {
			leave_page();
		}
	});
	page::listen(&window, "pointermove", |event: PointerEvent| {
		STATE
			.with_borrow_mut(|state| state.pointer = Some(Vec2::new(event.client_x() as f32, event.client_y() as f32)));
	});
	page::listen(&canvas, "pointerdown", |event: PointerEvent| {
		STATE.with_borrow_mut(|state| {
			state.pointer = Some(Vec2::new(event.client_x() as f32, event.client_y() as f32));
			match event.button() {
				0 => state.left = true,
				2 => state.right = true,
				_ => {}
			}
		});
	});
	page::listen(&window, "pointerup", |event: PointerEvent| {
		STATE.with_borrow_mut(|state| match event.button() {
			0 => state.left = false,
			2 => state.right = false,
			_ => {}
		});
	});
	page::listen_active(&canvas, "wheel", |event: WheelEvent| {
		event.prevent_default();
		STATE.with_borrow_mut(|state| state.wheel -= event.delta_y().signum() as f32);
	});
	// The right button launches a missile, so it must not open the menu of the browser.
	page::listen_active(&canvas, "contextmenu", |event: Event| event.prevent_default());
}

/// Forget every held key and button, because the page no longer sees their release.
fn leave_page() {
	STATE.with_borrow_mut(|state| {
		state.down.clear();
		(state.left, state.right, state.left_page) = (false, false, true);
	});
}

/// Tell the input whether a match takes the keys now.
pub fn set_in_flight(in_flight: bool) {
	STATE.with_borrow_mut(|state| state.in_flight = in_flight);
}

/// Take the codes of the keys that went down since the last call.
pub fn take_pressed() -> Vec<String> {
	STATE.with_borrow_mut(|state| std::mem::take(&mut state.pressed))
}

/// Return whether the page lost the focus or went out of view since the last call.
pub fn take_left_page() -> bool {
	STATE.with_borrow_mut(|state| std::mem::take(&mut state.left_page))
}

/// The part of the camera that turns a place on the screen into a direction.
#[derive(Clone, Copy, Debug)]
pub struct Lens {
	/// The tangent of half of the vertical angle of view.
	pub half_height: f32,
	/// The width of the picture for each unit of its height.
	pub aspect: f32,
}

/// Return the controls of the player for one step of `dt` seconds.
///
/// The plane with this `orientation` turns toward the place under the cursor, and the turn keys
/// and the nose keys move that place. `throttle` is the setting of the engine, which the wheel
/// and the throttle keys change.
pub fn read(options: &Options, orientation: Quat, lens: Lens, throttle: &mut f32, dt: f32) -> Controls {
	STATE.with_borrow_mut(|state| {
		let held = |bind: Bind| state.down.contains(options.key(bind));
		let axis = |more: Bind, less: Bind| f32::from(u8::from(held(more))) - f32::from(u8::from(held(less)));
		let window = page::window();
		let size = Vec2::new(
			window
				.inner_width()
				.ok()
				.and_then(|width| width.as_f64())
				.unwrap_or(1.0) as f32,
			window
				.inner_height()
				.ok()
				.and_then(|height| height.as_f64())
				.unwrap_or(1.0) as f32,
		);
		// The place of the cursor, from -1 at the left and the bottom to 1 at the right and the top.
		let cursor = state.pointer.map_or(Vec2::ZERO, |pointer| {
			Vec2::new(pointer.x / size.x * 2.0 - 1.0, 1.0 - pointer.y / size.y * 2.0)
		});
		let vertical = if options.invert { -cursor.y } else { cursor.y };
		let mouse =
			Vec2::new(cursor.x * lens.half_height * lens.aspect, vertical * lens.half_height) * options.sensitivity;
		let keys = Vec2::new(
			axis(Bind::TurnRight, Bind::TurnLeft),
			axis(Bind::NoseUp, Bind::NoseDown),
		) * KEY_AIM;
		*throttle = (*throttle
			+ std::mem::take(&mut state.wheel) * WHEEL_THROTTLE
			+ axis(Bind::ThrottleUp, Bind::ThrottleDown) * KEY_THROTTLE * dt)
			.clamp(0.0, 1.0);
		Controls {
			aim: aim_direction(orientation, mouse + keys),
			throttle: *throttle,
			boost: held(Bind::Boost),
			fire: state.left || held(Bind::Fire),
			launch: state.right || held(Bind::Launch),
		}
	})
}

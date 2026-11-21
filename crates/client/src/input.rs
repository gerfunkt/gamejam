//! The input of the player, as actions of the game.
//!
//! On a desktop, the mouse steers, and keys do the rest. On a phone, held sideways, a stick or
//! the tilt of the phone steers, and buttons on the screen do the rest. The page delivers the
//! events. The rules see only [`Controls`]. Each key can change, and the choices of the player
//! survive a visit.

use std::{cell::RefCell, collections::HashSet};

use bevy::math::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};
use sim::{Controls, Skill};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
	DeviceOrientationEvent, Element, Event, EventTarget, HtmlElement, KeyboardEvent, PointerEvent, WheelEvent,
};

use crate::page;

/// How much one notch of the wheel changes the throttle.
const WHEEL_THROTTLE: f32 = 0.1;
/// How much one second of a throttle key changes the throttle.
const KEY_THROTTLE: f32 = 0.7;
/// How far the stick at its limit moves the aim, as the tangent of the angle.
const STICK_AIM: f32 = 0.8;
/// How far the tilt at its limit moves the aim. A phone moves in the hand, so a gentler aim
/// keeps a small movement from throwing the plane across the sky.
const TILT_AIM: f32 = 0.5;
/// The name under which the page stores whether the tilt steers.
const TILT_KEY: &str = "tilt";
/// The tilt below which the phone counts as level, in radians.
const TILT_DEAD_ZONE: f32 = 0.035;
/// The tilt that gives the full turn, in radians.
const TILT_FULL: f32 = 0.49;
const STORAGE_KEY: &str = "dogfight-options";
/// The version of the stored options. Options of another version give way to the defaults.
const OPTIONS_VERSION: u32 = 3;

/// An action of the game that a key does. The mouse alone steers, so no key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bind {
	Fire,
	Launch,
	Flare,
	Boost,
	ThrottleUp,
	ThrottleDown,
	Pause,
}

impl Bind {
	pub const ALL: [Bind; 7] = [
		Bind::Fire,
		Bind::Launch,
		Bind::Flare,
		Bind::Boost,
		Bind::ThrottleUp,
		Bind::ThrottleDown,
		Bind::Pause,
	];

	/// The name of the action in the settings.
	pub fn label(self) -> &'static str {
		match self {
			Bind::Fire => "Fire the Gun",
			Bind::Launch => "Launch a Missile",
			Bind::Flare => "Release a Flare",
			Bind::Boost => "Boost",
			Bind::ThrottleUp => "More Throttle",
			Bind::ThrottleDown => "Less Throttle",
			Bind::Pause => "Pause Menu",
		}
	}

	/// The key of the action on a new visit, as the `code` of a keyboard event. A code names
	/// the place of a key, so the keys stay together on every keyboard layout.
	fn default_key(self) -> &'static str {
		match self {
			Bind::Fire => "Space",
			Bind::Launch => "KeyF",
			Bind::Flare => "KeyE",
			Bind::Boost => "ShiftLeft",
			Bind::ThrottleUp => "KeyW",
			Bind::ThrottleDown => "KeyS",
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
	/// Whether a mouse, a stick, or a tilt upward lowers the nose.
	pub invert: bool,
	/// Whether the display shows no flashes on a hit.
	#[serde(default)]
	pub calm: bool,
	/// The loudness of the sound effects, from 0 for silence to 1.
	#[serde(default = "default_volume")]
	pub effects_volume: f32,
	/// The loudness of the pilot voices, from 0 for silence to 1.
	#[serde(default = "default_volume")]
	pub voice_volume: f32,
	/// The key of each action, in the order of [`Bind::ALL`].
	keys: Vec<String>,
	/// The number of bots that the player chose last as a host.
	pub bots: u8,
	/// The skill of the bots that the player chose last as a host.
	pub skill: Skill,
}

fn default_volume() -> f32 {
	0.7
}

impl Default for Options {
	fn default() -> Self {
		Self {
			version: OPTIONS_VERSION,
			name: String::new(),
			sensitivity: 1.0,
			invert: false,
			calm: false,
			effects_volume: default_volume(),
			voice_volume: default_volume(),
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

/// A button of the phone layout that acts while a finger holds it.
#[derive(Clone, Copy)]
enum Hold {
	Fire,
	Launch,
	Flare,
	Boost,
}

/// The buttons that act while held, each with the id of its element.
const HOLDS: [(&str, Hold); 4] = [
	("touch-fire", Hold::Fire),
	("touch-launch", Hold::Launch),
	("touch-flare", Hold::Flare),
	("touch-boost", Hold::Boost),
];

/// The pose of the phone that counts as straight ahead.
#[derive(Clone, Copy)]
struct Level {
	/// The direction of the upper edge of the picture, in the frame of the phone.
	up: Vec3,
	/// The roll about the line of sight, in radians.
	roll: f32,
	/// The lean of the picture away from the player, in radians.
	lean: f32,
}

#[derive(Default)]
struct State {
	/// The codes of the keys that are down.
	down: HashSet<String>,
	/// The codes of the keys that went down since the last look.
	pressed: Vec<String>,
	/// The codes of the keys that went down since the last step of a match. A key that goes
	/// down and up between two steps still counts for one step.
	tapped: HashSet<String>,
	/// The place of the mouse in the page, in CSS pixels.
	pointer: Option<Vec2>,
	left: bool,
	right: bool,
	/// The notches that the wheel turned since the last look. Up is positive.
	wheel: f32,
	/// Whether the page lost the focus or went out of view since the last look.
	left_page: bool,
	/// Whether a match takes the keys now, so that the browser must not act on them.
	in_flight: bool,
	/// Whether the page shows the controls of a phone.
	touch: bool,
	/// The stick, from -1 at the left and the bottom to 1 at the right and the top.
	stick: Vec2,
	/// The finger that holds the stick, by the id of its pointer.
	stick_finger: Option<i32>,
	/// The buttons that a finger holds, in the order of [`HOLDS`], and those that a finger
	/// touched since the last step of a match.
	held: [bool; HOLDS.len()],
	touched: [bool; HOLDS.len()],
	/// A new place of the throttle slider.
	slid: Option<f32>,
	/// Whether a finger pressed the pause button since the last look.
	pause: bool,
	/// Whether the tilt of the phone steers.
	tilt: bool,
	/// Whether the page asked the browser for the motion sensor.
	asked: bool,
	/// The direction of gravity in the frame of the phone, from the last sensor event.
	gravity: Option<Vec3>,
	/// The pose that counts as straight ahead. The next sensor reading sets it when it is empty.
	level: Option<Level>,
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
				state.tapped.insert(event.code());
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
	// Only a mouse steers by its place. A finger on the sky steers nothing.
	let by_mouse = |event: &PointerEvent| event.pointer_type() == "mouse";
	page::listen(&window, "pointermove", move |event: PointerEvent| {
		if by_mouse(&event) {
			STATE.with_borrow_mut(|state| {
				state.pointer = Some(Vec2::new(event.client_x() as f32, event.client_y() as f32))
			});
		}
	});
	page::listen(&canvas, "pointerdown", move |event: PointerEvent| {
		if by_mouse(&event) {
			STATE.with_borrow_mut(|state| {
				state.pointer = Some(Vec2::new(event.client_x() as f32, event.client_y() as f32));
				match event.button() {
					0 => state.left = true,
					2 => state.right = true,
					_ => {}
				}
			});
		}
	});
	page::listen(&window, "pointerup", move |event: PointerEvent| {
		if by_mouse(&event) {
			STATE.with_borrow_mut(|state| match event.button() {
				0 => state.left = false,
				2 => state.right = false,
				_ => {}
			});
		}
	});
	page::listen_active(&canvas, "wheel", |event: WheelEvent| {
		event.prevent_default();
		STATE.with_borrow_mut(|state| state.wheel -= event.delta_y().signum() as f32);
	});
	// The right button launches a missile, so it must not open the menu of the browser.
	page::listen_active(&canvas, "contextmenu", |event: Event| event.prevent_default());

	if page::is_phone() {
		watch_phone(&window);
	}
}

/// Start to collect the input of the controls of a phone.
fn watch_phone(window: &EventTarget) {
	STATE.with_borrow_mut(|state| state.touch = true);
	if let Some(root) = page::document().document_element() {
		let _ = root.class_list().add_1("touch");
	}
	for (index, (id, _)) in HOLDS.into_iter().enumerate() {
		let button: EventTarget = page::element(id).into();
		page::listen_active(&button, "pointerdown", move |event: PointerEvent| {
			event.prevent_default();
			page::set_class(id, "held", true);
			STATE.with_borrow_mut(|state| (state.held[index], state.touched[index]) = (true, true));
		});
		for kind in ["pointerup", "pointercancel", "pointerleave"] {
			page::listen(&button, kind, move |_: PointerEvent| {
				page::set_class(id, "held", false);
				STATE.with_borrow_mut(|state| state.held[index] = false);
			});
		}
	}

	// The stick follows the finger that touched it, also when the finger leaves its circle.
	let stick = page::element("stick");
	let follow = |event: &PointerEvent| {
		let stick: HtmlElement = page::element("stick");
		let bounds = stick.get_bounding_client_rect();
		let reach = (bounds.width() / 2.0) as f32;
		let centre = Vec2::new(
			(bounds.left() + bounds.width() / 2.0) as f32,
			(bounds.top() + bounds.height() / 2.0) as f32,
		);
		let pull = ((Vec2::new(event.client_x() as f32, event.client_y() as f32) - centre) / reach.max(1.0))
			.clamp_length_max(1.0);
		page::set_style(
			"knob",
			"transform",
			&format!("translate({:.1}px,{:.1}px)", pull.x * reach * 0.6, pull.y * reach * 0.6),
		);
		STATE.with_borrow_mut(|state| state.stick = Vec2::new(pull.x, -pull.y));
	};
	let target: EventTarget = stick.into();
	page::listen_active(&target, "pointerdown", move |event: PointerEvent| {
		event.prevent_default();
		STATE.with_borrow_mut(|state| state.stick_finger = Some(event.pointer_id()));
		follow(&event);
	});
	page::listen(window, "pointermove", move |event: PointerEvent| {
		if STATE.with_borrow(|state| state.stick_finger == Some(event.pointer_id())) {
			follow(&event);
		}
	});
	for kind in ["pointerup", "pointercancel"] {
		page::listen(window, kind, |event: PointerEvent| {
			let released = STATE.with_borrow_mut(|state| {
				let released = state.stick_finger == Some(event.pointer_id());
				if released {
					(state.stick_finger, state.stick) = (None, Vec2::ZERO);
				}
				released
			});
			if released {
				page::set_style("knob", "transform", "translate(0px,0px)");
			}
		});
	}

	page::listen(&page::element("touch-throttle").into(), "input", |_: Event| {
		let slid = page::field("touch-throttle").parse().ok();
		STATE.with_borrow_mut(|state| state.slid = slid);
	});
	page::listen(&page::element("touch-pause").into(), "click", |_: Event| {
		STATE.with_borrow_mut(|state| state.pause = true);
	});
	page::listen(&page::element("touch-centre").into(), "click", |_: Event| {
		STATE.with_borrow_mut(|state| state.level = None);
	});
	// The tilt steers from the start, unless the player turned it off at an earlier visit.
	set_tilt(page::load(TILT_KEY).as_deref() != Some("off"));
	// The browser of an iPhone gives the sensor only on a request that comes straight from a tap,
	// so these listeners ask, and do not wait for the next frame of the game. The first tap on
	// the page asks for the tilt that is on from the start.
	page::listen(window, "click", |_: Event| {
		let ask = STATE.with_borrow_mut(|state| state.tilt && !std::mem::replace(&mut state.asked, true));
		if ask {
			ask_for_sensor(|granted| {
				if !granted {
					set_tilt(false);
				}
			});
		}
	});
	page::listen(&page::element("touch-tilt").into(), "click", |_: Event| {
		let on = STATE.with_borrow_mut(|state| {
			state.asked = true;
			!state.tilt
		});
		page::store(TILT_KEY, if on { "on" } else { "off" });
		if on {
			ask_for_sensor(set_tilt);
		} else {
			set_tilt(false);
		}
	});
	page::listen(window, "deviceorientation", |event: DeviceOrientationEvent| {
		let (Some(beta), Some(gamma)) = (event.beta(), event.gamma()) else {
			return;
		};
		let (beta, gamma) = ((beta as f32).to_radians(), (gamma as f32).to_radians());
		// The angles of the event, as the direction of gravity in the frame of the phone: X to
		// the right of the upright phone, Y to its top, and Z out of the screen.
		let gravity = Vec3::new(beta.cos() * gamma.sin(), -beta.sin(), -beta.cos() * gamma.cos());
		STATE.with_borrow_mut(|state| state.gravity = Some(gravity));
	});
}

/// Turn the tilt steering on or off, and show it on its button.
fn set_tilt(on: bool) {
	STATE.with_borrow_mut(|state| (state.tilt, state.level) = (on, None));
	let _ = page::element("touch-tilt").set_attribute("aria-pressed", if on { "true" } else { "false" });
	page::set_hidden("touch-centre", !on);
}

/// Ask the browser for the motion sensor, where it wants to be asked, and tell `answer` whether it agrees.
///
/// Most browsers give the sensor without a question, and that counts as an agreement.
fn ask_for_sensor(answer: impl FnOnce(bool) + 'static) {
	let request = js_sys::Reflect::get(&page::window(), &"DeviceOrientationEvent".into())
		.and_then(|sensor| js_sys::Reflect::get(&sensor, &"requestPermission".into()).map(|request| (sensor, request)));
	let Ok((sensor, request)) = request else {
		return answer(true);
	};
	let Some(request) = request.dyn_ref::<js_sys::Function>() else {
		return answer(true);
	};
	let Ok(asked) = request.call0(&sensor) else {
		return answer(false);
	};
	let Ok(asked) = asked.dyn_into::<js_sys::Promise>() else {
		return answer(true);
	};
	wasm_bindgen_futures::spawn_local(async move {
		answer(wasm_bindgen_futures::JsFuture::from(asked).await == Ok(JsValue::from_str("granted")));
	});
}

/// Return how far the phone leans away from its level pose, as a place of a stick.
///
/// A roll to the right steers right. A lean of the top away from the player lowers the nose, as
/// a push on a stick does.
fn tilt(gravity: Vec3, level: &mut Option<Level>) -> Vec2 {
	// The upper edge of the picture is the side of the phone that points most against gravity.
	let up = level.map_or_else(
		|| {
			if gravity.x.abs() > gravity.y.abs() {
				Vec3::X * -gravity.x.signum()
			} else {
				Vec3::Y * -gravity.y.signum()
			}
		},
		|level| level.up,
	);
	let right = up.cross(Vec3::Z);
	let roll = gravity.dot(right).atan2(-gravity.dot(up));
	let lean = (-gravity.z).clamp(-1.0, 1.0).asin();
	let level = level.get_or_insert(Level { up, roll, lean });
	let curve =
		|angle: f32| angle.signum() * ((angle.abs() - TILT_DEAD_ZONE) / (TILT_FULL - TILT_DEAD_ZONE)).clamp(0.0, 1.0);
	Vec2::new(curve(roll - level.roll), -curve(lean - level.lean))
}

/// Forget every held key and button, because the page no longer sees their release.
fn leave_page() {
	STATE.with_borrow_mut(|state| {
		state.down.clear();
		(state.left, state.right, state.left_page) = (false, false, true);
		(state.held, state.stick, state.stick_finger) = ([false; HOLDS.len()], Vec2::ZERO, None);
	});
}

/// Tell the input whether a match takes the keys now.
///
/// The pose in which a phone enters the flight counts as straight ahead for its tilt.
pub fn set_in_flight(in_flight: bool) {
	STATE.with_borrow_mut(|state| {
		if in_flight && !state.in_flight {
			state.level = None;
		}
		state.in_flight = in_flight;
	});
}

/// Return whether the page shows the controls of a phone.
pub fn is_touch() -> bool {
	STATE.with_borrow(|state| state.touch)
}

/// Take the codes of the keys that went down since the last call.
pub fn take_pressed() -> Vec<String> {
	STATE.with_borrow_mut(|state| std::mem::take(&mut state.pressed))
}

/// Return whether a finger pressed the pause button of the phone layout since the last call.
pub fn take_pause() -> bool {
	STATE.with_borrow_mut(|state| std::mem::take(&mut state.pause))
}

/// Return whether the page lost the focus or went out of view since the last call.
pub fn take_left_page() -> bool {
	STATE.with_borrow_mut(|state| std::mem::take(&mut state.left_page))
}

/// The camera, as the aim needs it.
#[derive(Clone, Copy, Debug)]
pub struct Lens {
	/// The rotation of the camera. The aim lies in this frame, so it is the place under the cursor.
	pub frame: Quat,
	/// The tangent of half of the vertical angle of view.
	pub half_height: f32,
	/// The width of the picture for each unit of its height.
	pub aspect: f32,
}

/// Return the controls of the player for one step of `dt` seconds.
///
/// The plane turns toward the place under the mouse cursor, or toward the place that the stick
/// or the tilt of a phone gives. `throttle` is the setting of the engine, which the wheel, the
/// throttle keys, and the slider of a phone change.
pub fn read(options: &Options, lens: Lens, throttle: &mut f32, dt: f32) -> Controls {
	STATE.with_borrow_mut(|state| {
		let tapped = std::mem::take(&mut state.tapped);
		let touched = std::mem::take(&mut state.touched);
		let key = |bind: Bind| state.down.contains(options.key(bind)) || tapped.contains(options.key(bind));
		let button = |hold: Hold| state.held[hold as usize] || touched[hold as usize];
		let axis = |more: Bind, less: Bind| f32::from(u8::from(key(more))) - f32::from(u8::from(key(less)));

		let steer = if state.touch {
			let leaned = match (state.tilt, state.gravity) {
				(true, Some(gravity)) => tilt(gravity, &mut state.level),
				_ => Vec2::ZERO,
			};
			(state.stick * STICK_AIM + leaned * TILT_AIM).clamp_length_max(STICK_AIM)
		} else {
			let window = page::window();
			let side =
				|length: Result<JsValue, JsValue>| length.ok().and_then(|length| length.as_f64()).unwrap_or(1.0) as f32;
			let size = Vec2::new(side(window.inner_width()), side(window.inner_height()));
			// The place of the cursor, from -1 at the left and the bottom to 1 at the right and the top.
			let cursor = state.pointer.map_or(Vec2::ZERO, |pointer| {
				Vec2::new(pointer.x / size.x * 2.0 - 1.0, 1.0 - pointer.y / size.y * 2.0)
			});
			Vec2::new(cursor.x * lens.half_height * lens.aspect, cursor.y * lens.half_height) * options.sensitivity
		};
		let steer = if options.invert {
			Vec2::new(steer.x, -steer.y)
		} else {
			steer
		};

		if let Some(slid) = state.slid.take() {
			*throttle = slid;
		}
		*throttle = (*throttle
			+ std::mem::take(&mut state.wheel) * WHEEL_THROTTLE
			+ axis(Bind::ThrottleUp, Bind::ThrottleDown) * KEY_THROTTLE * dt)
			.clamp(0.0, 1.0);
		Controls {
			aim: lens.frame * Vec3::new(steer.x, steer.y, -1.0).normalize(),
			throttle: *throttle,
			boost: key(Bind::Boost) || button(Hold::Boost),
			fire: state.left || key(Bind::Fire) || button(Hold::Fire),
			launch: state.right || key(Bind::Launch) || button(Hold::Launch),
			flare: key(Bind::Flare) || button(Hold::Flare),
		}
	})
}

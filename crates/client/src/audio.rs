//! The sounds of a match, through the Web Audio interface of the browser.
//!
//! The files and their names come from `assets/audio/manifest.json` beside the page. A browser
//! lets a page play sound only after the player acted, so the sound starts with the first
//! click. A sound that is not loaded yet stays silent, and nothing else waits for it.

use std::{cell::RefCell, collections::HashMap};

use bevy::prelude::*;
use serde::Deserialize;
use sim::{Event, Part, Session, tuning};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{AudioBuffer, AudioBufferSourceNode, AudioContext, GainNode};

use crate::{
	flow::{Flow, Game, Screen},
	page,
};

/// The distance at which a sound in the sky is no longer heard, in metres.
const HEARING_RANGE: f32 = 1600.0;
/// The shortest time between two calls of the same voice line, in seconds.
const VOICE_PAUSE: f64 = 9.0;
/// The time between two ticks of the seeker while a lock closes, in seconds.
const TICK_INTERVAL: f32 = 0.3;

/// The list of sounds that the build of the audio assets wrote.
#[derive(Deserialize)]
struct Manifest {
	foley: Vec<Entry>,
	voice: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
	id: String,
	file: String,
}

struct Audio {
	context: AudioContext,
	effects: GainNode,
	voices: GainNode,
	buffers: HashMap<String, AudioBuffer>,
	/// The sounds that repeat, each with the node that sets its loudness.
	loops: HashMap<&'static str, (AudioBufferSourceNode, GainNode)>,
	/// The time of the audio clock until which a voice line speaks.
	voice_until: f64,
	/// The time of the audio clock at which each voice line spoke last.
	spoken: HashMap<&'static str, f64>,
	/// The number of sounds that started, for the browser tests.
	started: u32,
}

thread_local! {
	static AUDIO: RefCell<Option<Audio>> = const { RefCell::new(None) };
}

/// Start the sound, and load the files. Call this after the player acted, and as often as that happens.
pub fn start() {
	if AUDIO.with_borrow(Option::is_some) {
		return;
	}
	let Ok(context) = AudioContext::new() else { return };
	let (Ok(effects), Ok(voices)) = (context.create_gain(), context.create_gain()) else {
		return;
	};
	for bus in [&effects, &voices] {
		let _ = bus.connect_with_audio_node(&context.destination());
	}
	AUDIO.with_borrow_mut(|audio| {
		*audio = Some(Audio {
			context,
			effects,
			voices,
			buffers: HashMap::new(),
			loops: HashMap::new(),
			voice_until: 0.0,
			spoken: HashMap::new(),
			started: 0,
		});
	});
	page::fetch_text("assets/audio/manifest.json", |text| {
		let Some(manifest) = text.and_then(|text| serde_json::from_str::<Manifest>(&text).ok()) else {
			return;
		};
		for entry in manifest.foley.into_iter().chain(manifest.voice) {
			wasm_bindgen_futures::spawn_local(async move {
				if let Some(buffer) = load(&entry.file).await {
					AUDIO.with_borrow_mut(|audio| audio.as_mut().map(|audio| audio.buffers.insert(entry.id, buffer)));
				}
			});
		}
	});
}

/// Fetch one sound file, and decode it.
async fn load(file: &str) -> Option<AudioBuffer> {
	let response = JsFuture::from(page::window().fetch_with_str(&format!("assets/audio/{file}")))
		.await
		.ok()?;
	let response: web_sys::Response = response.dyn_into().ok()?;
	let bytes: js_sys::ArrayBuffer = JsFuture::from(response.array_buffer().ok()?)
		.await
		.ok()?
		.dyn_into()
		.ok()?;
	let decoding = AUDIO.with_borrow(|audio| audio.as_ref()?.context.decode_audio_data(&bytes).ok())?;
	JsFuture::from(decoding).await.ok()?.dyn_into().ok()
}

impl Audio {
	/// Start a sound once on a bus, and return its length in seconds.
	fn start(&mut self, id: &str, loudness: f32, voice: bool) -> Option<f64> {
		let buffer = self.buffers.get(id)?;
		let source = self.context.create_buffer_source().ok()?;
		source.set_buffer(Some(buffer));
		let gain = self.context.create_gain().ok()?;
		gain.gain().set_value(loudness);
		source.connect_with_audio_node(&gain).ok()?;
		gain.connect_with_audio_node(if voice { &self.voices } else { &self.effects })
			.ok()?;
		source.start().ok()?;
		self.started += 1;
		Some(buffer.duration())
	}
}

/// Play an effect once, at a loudness from 0 to 1.
pub fn play(id: &str, loudness: f32) {
	if loudness > 0.01 {
		AUDIO.with_borrow_mut(|audio| audio.as_mut().and_then(|audio| audio.start(id, loudness, false)));
	}
}

/// Speak a voice line, unless another line speaks now, or this line spoke a moment ago.
pub fn say(id: &'static str) {
	AUDIO.with_borrow_mut(|audio| {
		let Some(audio) = audio else { return };
		let now = audio.context.current_time();
		if now < audio.voice_until || audio.spoken.get(id).is_some_and(|last| now < last + VOICE_PAUSE) {
			return;
		}
		if let Some(length) = audio.start(id, 1.0, true) {
			audio.voice_until = now + length;
			audio.spoken.insert(id, now);
		}
	});
}

/// Keep a sound that repeats at this loudness and this speed. A loudness of 0 stops it.
pub fn keep(id: &'static str, loudness: f32, speed: f32) {
	AUDIO.with_borrow_mut(|audio| {
		let Some(audio) = audio else { return };
		if loudness <= 0.0 {
			if let Some((source, _)) = audio.loops.remove(id) {
				// The stop of the node type itself is deprecated in favour of the one of its parent type.
				let scheduled: &web_sys::AudioScheduledSourceNode = &source;
				let _ = scheduled.stop();
			}
			return;
		}
		if !audio.loops.contains_key(id) {
			let (Some(buffer), Ok(source), Ok(gain)) = (
				audio.buffers.get(id),
				audio.context.create_buffer_source(),
				audio.context.create_gain(),
			) else {
				return;
			};
			source.set_buffer(Some(buffer));
			source.set_loop(true);
			let wired =
				source.connect_with_audio_node(&gain).is_ok() && gain.connect_with_audio_node(&audio.effects).is_ok();
			if !wired || source.start().is_err() {
				return;
			}
			audio.started += 1;
			audio.loops.insert(id, (source, gain));
		}
		if let Some((source, gain)) = audio.loops.get(id) {
			gain.gain().set_value(loudness);
			source.playback_rate().set_value(speed);
		}
	});
}

/// Set the loudness of the effects and of the voices, each from 0 for silence to 1.
pub fn set_volumes(effects: f32, voices: f32) {
	AUDIO.with_borrow(|audio| {
		if let Some(audio) = audio {
			audio.effects.gain().set_value(effects);
			audio.voices.gain().set_value(voices);
		}
	});
}

/// Return the number of sounds that started since the page loaded.
pub fn started() -> u32 {
	AUDIO.with_borrow(|audio| audio.as_ref().map_or(0, |audio| audio.started))
}

pub fn plugin(app: &mut App) {
	app.add_systems(Update, play_match.after(Flow));
}

/// What the sound remembers of the last frame, to hear what changed.
#[derive(Default)]
struct Heard {
	in_match: bool,
	alive: bool,
	boosting: bool,
	firing: f32,
	missiles: u8,
	lock: f32,
	tick: f32,
	threatened: bool,
}

/// Return how loud a sound at `place` is for a listener at `ear`, from 0 to 1.
fn nearness(place: Vec3, ear: Vec3) -> f32 {
	(1.0 - place.distance(ear) / HEARING_RANGE).max(0.0).powi(2)
}

/// Play the sounds of the match: what happened in this frame, and what the plane of the player does.
fn play_match(game: Res<Game>, time: Res<Time>, mut heard: Local<Heard>) {
	set_volumes(game.options.effects_volume, game.options.voice_volume);
	let running = game
		.session
		.as_ref()
		.and_then(Session::game)
		.filter(|_| game.screen == Screen::Playing);
	let Some(running) = running else {
		for id in ["engine_loop", "wind_rush", "lock_tone"] {
			keep(id, 0.0, 1.0);
		}
		*heard = Heard::default();
		return;
	};
	let me = running.me();
	let Some(own) = running.plane(me) else { return };
	let planes = running.planes();
	let alive = running.respawn_in(me).is_none();
	if !heard.in_match {
		say("match_start");
	} else if alive && !heard.alive {
		say("back_in_the_air");
	}

	for event in &game.events {
		match event {
			Event::Hit { by, .. } if *by == me => play("hit_metal", 0.35),
			Event::Damage { plane, part, broke } if *plane == me => {
				play("hit_metal", 0.8);
				if *broke {
					play("part_break", 0.9);
					say(match part {
						Part::LeftWing | Part::RightWing => "wing_lost",
						Part::Tail => "tail_lost",
						Part::Engine => "engine_out",
						Part::Fuselage => "going_down",
					});
				} else {
					say("taking_fire");
				}
			}
			Event::Kill { victim, killer } => {
				let place = planes
					.iter()
					.find(|(id, _)| id == victim)
					.map_or(own.position, |(_, state)| state.position);
				play(
					"plane_kill",
					nearness(place, own.position).max(if *victim == me { 1.0 } else { 0.0 }),
				);
				if *killer == Some(me) {
					say("splash_one");
				} else if *victim == me {
					say("going_down");
				}
			}
			Event::Burst { position } => play("missile_burst", nearness(*position, own.position)),
			_ => {}
		}
	}

	// The gun, the missiles, and the boost of the player.
	let dt = time.delta_secs();
	let state = planes.iter().find(|(id, _)| *id == me).map(|(_, state)| *state);
	let firing = alive && state.is_some_and(|state| state.firing);
	heard.firing = if firing { heard.firing - dt } else { 0.0 };
	if firing && heard.firing <= 0.0 {
		play("gun_shot", 0.3);
		if heard.firing == 0.0 {
			say("guns_guns_guns");
		}
		heard.firing += tuning::FIRE_INTERVAL;
	}
	let missiles = running.missiles_ready(me).unwrap_or(0);
	if missiles < heard.missiles {
		play("missile_launch", 0.8);
		say("fox_two");
	}
	if own.is_boosting() && !heard.boosting {
		play("boost_ignite", 0.6);
	}

	// The seeker: a tick while the lock closes, and a tone when it holds.
	let lock = if alive {
		running.lock(me).map_or(0.0, |(_, closed)| closed)
	} else {
		0.0
	};
	heard.tick -= dt;
	if lock > 0.0 && lock < 1.0 && heard.tick <= 0.0 {
		play("lock_tick", 0.4);
		heard.tick = TICK_INTERVAL;
	}
	if lock >= 1.0 && heard.lock < 1.0 {
		say("target_locked");
	}
	keep("lock_tone", if lock >= 1.0 { 0.18 } else { 0.0 }, 1.0);

	// A missile that follows the player.
	let threatened = running
		.missiles()
		.iter()
		.any(|missile| missile.target == me && missile.is_seeking() && !missile.is_lured());
	if threatened && !heard.threatened {
		say("missile_incoming");
	}

	// The engine and the wind follow the speed.
	let pace = own.speed / tuning::BOOST_SPEED;
	keep(
		"engine_loop",
		if alive { 0.12 + 0.2 * pace } else { 0.0 },
		0.7 + 0.7 * pace,
	);
	keep(
		"wind_rush",
		if alive { 0.25 * pace * pace } else { 0.0 },
		0.8 + 0.4 * pace,
	);

	*heard = Heard {
		in_match: true,
		alive,
		boosting: own.is_boosting(),
		firing: heard.firing,
		missiles,
		lock,
		tick: heard.tick,
		threatened,
	};
}

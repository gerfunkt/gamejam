//! The sounds of a match, through the Web Audio interface of the browser.
//!
//! The match raises named events, and two lists beside the page decide the sounds: the `foley`
//! and `voice` entries of `assets/audio/manifest.json`, and the `event_map` of
//! `assets/audio/audio_catalog.json`. The `events` module raises the events. This module holds
//! the table of the sounds of the first pack, and plays the sounds.
//!
//! A browser lets a page play sound only after the player acted, so the sound starts with the
//! first click. A sound that is not loaded yet, or that fails to load, stays silent.

mod events;

use std::{
	cell::RefCell,
	cmp::Reverse,
	collections::{HashMap, HashSet},
};

use bevy::prelude::*;
use serde::Deserialize;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{AudioBuffer, AudioBufferSourceNode, AudioContext, AudioScheduledSourceNode, GainNode};

use self::events::{Cue, Cues};
use crate::{flow::Flow, page};

/// How long a line waits for its speaker, in seconds.
const QUEUE_WAIT: f64 = 2.0;
/// How many lines wait for one speaker.
const QUEUE_LENGTH: usize = 3;
/// The shortest time between two calls of a radio line, which the manifest gives no pause, in seconds.
const RADIO_PAUSE: f64 = 9.0;

pub fn plugin(app: &mut App) {
	app.add_systems(Update, events::play_match.after(Flow));
}

// ---------------------------------------------------------------------------------------------
// The sounds of the first pack, which the manifest lists.

/// The radio lines that stay, each with its priority. The radio says the other lines of the manifest no more.
const RADIO: [(&str, u32); 7] = [
	("going_down", 80),
	("fox_two", 60),
	("splash_one", 60),
	("back_in_the_air", 55),
	("match_start", 55),
	("taking_fire", 50),
	("guns_guns_guns", 40),
];
/// The made effects that stay.
const MADE: [&str; 6] = [
	"hit_metal",
	"part_break",
	"plane_kill",
	"wind_rush",
	"boost_ignite",
	"ui_click",
];
/// The events of the first pack, each with a sound of the manifest. They add to the `event_map` of the catalog.
///
/// The made effects for the gun, the launch, the burst, and the lock have no entry: the catalog has these.
const FIRST_PACK: [(&str, &str); 17] = [
	("match_start", "match_start"),
	("missile_launched_by_me", "fox_two"),
	("kill_scored", "splash_one"),
	("gun_start", "guns_guns_guns"),
	("damage_survived", "taking_fire"),
	("player_down", "going_down"),
	("respawned", "back_in_the_air"),
	("hit_landed", "hit_metal"),
	("hit_taken", "hit_metal"),
	("part_broken_left_wing", "part_break"),
	("part_broken_right_wing", "part_break"),
	("part_broken_tail", "part_break"),
	("part_broken_engine", "part_break"),
	("plane_killed", "plane_kill"),
	("boost_start", "boost_ignite"),
	("wind_rush", "wind_rush"),
	("ui_click", "ui_click"),
];
/// The sounds of the catalog that repeat while a state lasts.
const LOOPS: [&str; 3] = ["sfx_aim9_growl", "sfx_engine_cockpit", "sfx_exhaust_afterburner"];

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
	#[serde(default, rename = "loop")]
	looped: bool,
}

/// The sounds of the flight simulators, and the events that play them.
#[derive(Deserialize)]
struct Catalog {
	event_map: HashMap<String, Ids>,
	assets: Vec<Asset>,
}

/// The sounds of one event: one id, or a list.
#[derive(Deserialize)]
#[serde(untagged)]
enum Ids {
	One(String),
	Many(Vec<String>),
}

#[derive(Deserialize)]
struct Asset {
	id: String,
	category: String,
	url: String,
	priority: u32,
	cooldown_s: f64,
}

// ---------------------------------------------------------------------------------------------
// The playing: effects, loops, and the two speakers.

/// A speaker of voice lines. It says one line at a time.
#[derive(Clone, Copy, PartialEq)]
enum Voice {
	/// The pilots, on the radio.
	Radio,
	/// The voice warning system of the plane.
	Betty,
}

/// How a sound plays.
#[derive(Clone, Copy, PartialEq)]
enum Role {
	/// Once. Several can overlap.
	Effect,
	/// Again and again, while a state lasts.
	Loop,
	/// A line of one speaker.
	Say(Voice),
}

struct Sound {
	role: Role,
	/// A line with a higher priority cuts off a line with a lower one.
	priority: u32,
	/// The shortest time between two starts of the sound, in seconds.
	cooldown: f64,
	/// The decoded file. It is missing until the file loads, and for good if it fails.
	buffer: Option<AudioBuffer>,
}

/// The line that a speaker says now.
struct Saying {
	source: AudioBufferSourceNode,
	priority: u32,
	/// The time of the audio clock at which the line ends.
	until: f64,
}

/// A line that waits for its speaker, and the time at which it began to wait.
struct Waiting {
	id: String,
	since: f64,
}

#[derive(Default)]
struct Speaker {
	saying: Option<Saying>,
	queue: Vec<Waiting>,
}

struct Audio {
	context: AudioContext,
	effects: GainNode,
	voices: GainNode,
	/// The sounds by id. The ids of the manifest and of the catalog differ.
	sounds: HashMap<String, Sound>,
	/// The ids of the sounds of each event.
	events: HashMap<String, Vec<String>>,
	/// The loops that play, each with the node that sets its loudness.
	loops: HashMap<String, (AudioBufferSourceNode, GainNode)>,
	/// The radio and Betty, in this order.
	speakers: [Speaker; 2],
	/// The time of the audio clock at which each sound started last.
	last: HashMap<String, f64>,
	/// How often each event chose a line, to choose the next line in turn.
	turns: HashMap<String, usize>,
	/// The time of the audio clock at which each state may raise its event again.
	repeats: HashMap<String, f64>,
	/// The number of sounds that started, for the browser tests.
	started: u32,
	/// Whether to write a line for each event, for the browser tests.
	log: bool,
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
	let mut events: HashMap<String, Vec<String>> = HashMap::new();
	for (event, id) in FIRST_PACK {
		events.entry(event.to_owned()).or_default().push(id.to_owned());
	}
	AUDIO.with_borrow_mut(|audio| {
		*audio = Some(Audio {
			context,
			effects,
			voices,
			sounds: HashMap::new(),
			events,
			loops: HashMap::new(),
			speakers: [Speaker::default(), Speaker::default()],
			last: HashMap::new(),
			turns: HashMap::new(),
			repeats: HashMap::new(),
			started: 0,
			log: page::query("stat").as_deref() == Some("1"),
		});
	});
	page::fetch_text("assets/audio/manifest.json", |text| {
		if let Some(manifest) = text.and_then(|text| serde_json::from_str::<Manifest>(&text).ok()) {
			for entry in manifest.foley.into_iter().chain(manifest.voice) {
				let (role, priority, cooldown) =
					if let Some((_, priority)) = RADIO.iter().find(|(id, _)| *id == entry.id) {
						(Role::Say(Voice::Radio), *priority, RADIO_PAUSE)
					} else if MADE.contains(&entry.id.as_str()) {
						(if entry.looped { Role::Loop } else { Role::Effect }, 0, 0.0)
					} else {
						continue;
					};
				register(
					entry.id,
					format!("assets/audio/{}", entry.file),
					role,
					priority,
					cooldown,
				);
			}
		}
	});
	page::fetch_text("assets/audio/audio_catalog.json", |text| {
		let Some(catalog) = text.and_then(|text| serde_json::from_str::<Catalog>(&text).ok()) else {
			return;
		};
		let mut used = HashSet::new();
		AUDIO.with_borrow_mut(|audio| {
			let Some(audio) = audio else { return };
			for (event, ids) in catalog.event_map {
				let ids = match ids {
					Ids::One(id) => vec![id],
					Ids::Many(ids) => ids,
				};
				used.extend(ids.iter().cloned());
				audio.events.entry(event).or_default().extend(ids);
			}
		});
		// A sound that no event names is never heard, so its file stays on the server.
		for asset in catalog.assets.into_iter().filter(|asset| used.contains(&asset.id)) {
			let role = match asset.category.as_str() {
				"vo/betty" => Role::Say(Voice::Betty),
				"sfx" if LOOPS.contains(&asset.id.as_str()) => Role::Loop,
				"sfx" => Role::Effect,
				_ => continue,
			};
			// The page may not be at the root of its server, so the path is relative to the page.
			let path = asset.url.trim_start_matches('/').to_owned();
			register(asset.id, path, role, asset.priority, asset.cooldown_s);
		}
	});
}

/// Add a sound to the list, and load its file in the background.
fn register(id: String, path: String, role: Role, priority: u32, cooldown: f64) {
	AUDIO.with_borrow_mut(|audio| {
		if let Some(audio) = audio {
			audio.sounds.insert(
				id.clone(),
				Sound {
					role,
					priority,
					cooldown,
					buffer: None,
				},
			);
		}
	});
	wasm_bindgen_futures::spawn_local(async move {
		if let Some(buffer) = load(&path).await {
			AUDIO.with_borrow_mut(|audio| {
				if let Some(sound) = audio.as_mut().and_then(|audio| audio.sounds.get_mut(&id)) {
					sound.buffer = Some(buffer);
				}
			});
		}
	});
}

/// Fetch one sound file, and decode it.
async fn load(path: &str) -> Option<AudioBuffer> {
	let response = JsFuture::from(page::window().fetch_with_str(path)).await.ok()?;
	let response: web_sys::Response = response.dyn_into().ok()?;
	if !response.ok() {
		return None;
	}
	let bytes: js_sys::ArrayBuffer = JsFuture::from(response.array_buffer().ok()?)
		.await
		.ok()?
		.dyn_into()
		.ok()?;
	let decoding = AUDIO.with_borrow(|audio| audio.as_ref()?.context.decode_audio_data(&bytes).ok())?;
	JsFuture::from(decoding).await.ok()?.dyn_into().ok()
}

/// Stop a source. The stop of the node type itself is deprecated in favour of the one of its parent type.
fn stop(source: &AudioBufferSourceNode) {
	let scheduled: &AudioScheduledSourceNode = source;
	let _ = scheduled.stop();
}

impl Audio {
	fn now(&self) -> f64 {
		self.context.current_time()
	}

	/// Return whether the sound is loaded, and its pause since its last start is over.
	fn ready(&self, id: &str, now: f64) -> bool {
		self.sounds.get(id).is_some_and(|sound| {
			sound.buffer.is_some() && self.last.get(id).is_none_or(|last| now >= last + sound.cooldown)
		})
	}

	/// Start a sound once on a bus, and return its source and its length in seconds.
	fn launch(&mut self, id: &str, loudness: f32, voice: bool) -> Option<(AudioBufferSourceNode, f64)> {
		let buffer = self.sounds.get(id)?.buffer.as_ref()?;
		let source = self.context.create_buffer_source().ok()?;
		source.set_buffer(Some(buffer));
		let gain = self.context.create_gain().ok()?;
		gain.gain().set_value(loudness);
		source.connect_with_audio_node(&gain).ok()?;
		gain.connect_with_audio_node(if voice { &self.voices } else { &self.effects })
			.ok()?;
		source.start().ok()?;
		let length = buffer.duration();
		self.started += 1;
		Some((source, length))
	}

	/// Play an effect once. Return whether it started.
	fn play_effect(&mut self, id: &str, loudness: f32, now: f64) -> bool {
		let began = self.ready(id, now) && self.launch(id, loudness, false).is_some();
		if began {
			self.last.insert(id.to_owned(), now);
		}
		began
	}

	/// Let a speaker say a line. Return whether it started now: a line behind another one waits.
	fn say(&mut self, voice: Voice, id: &str, now: f64) -> bool {
		if !self.ready(id, now) {
			return false;
		}
		let priority = self.sounds.get(id).map_or(0, |sound| sound.priority);
		let speaker = &mut self.speakers[voice as usize];
		let busy = speaker.saying.as_ref().filter(|saying| saying.until > now);
		if busy.is_some_and(|saying| priority <= saying.priority) {
			if !speaker.queue.iter().any(|waiting| waiting.id == id) {
				speaker.queue.push(Waiting {
					id: id.to_owned(),
					since: now,
				});
				if speaker.queue.len() > QUEUE_LENGTH {
					speaker.queue.remove(0);
				}
			}
			return false;
		}
		if let Some(saying) = speaker.saying.take() {
			stop(&saying.source);
		}
		let Some((source, length)) = self.launch(id, 1.0, true) else {
			return false;
		};
		self.speakers[voice as usize].saying = Some(Saying {
			source,
			priority,
			until: now + length,
		});
		self.last.insert(id.to_owned(), now);
		true
	}

	/// Let each speaker that is free say the line of its queue that matters most. Drop the lines that waited too long.
	fn pump(&mut self, now: f64) {
		for voice in [Voice::Radio, Voice::Betty] {
			let speaker = &mut self.speakers[voice as usize];
			speaker.queue.retain(|waiting| now - waiting.since <= QUEUE_WAIT);
			if speaker.saying.as_ref().is_some_and(|saying| saying.until > now) {
				continue;
			}
			let sounds = &self.sounds;
			let next = speaker
				.queue
				.iter()
				.enumerate()
				.max_by_key(|(place, waiting)| {
					(
						sounds.get(&waiting.id).map_or(0, |sound| sound.priority),
						Reverse(*place),
					)
				})
				.map(|(place, _)| place);
			if let Some(place) = next {
				let waiting = speaker.queue.remove(place);
				self.say(voice, &waiting.id, now);
			}
		}
	}

	/// Keep a loop at this loudness and this speed. A loudness of 0 stops it. Return whether it started now.
	fn hold(&mut self, id: &str, loudness: f32, speed: f32) -> bool {
		if loudness <= 0.0 {
			if let Some((source, _)) = self.loops.remove(id) {
				stop(&source);
			}
			return false;
		}
		let mut began = false;
		if !self.loops.contains_key(id) {
			let (Some(buffer), Ok(source), Ok(gain)) = (
				self.sounds.get(id).and_then(|sound| sound.buffer.as_ref()),
				self.context.create_buffer_source(),
				self.context.create_gain(),
			) else {
				return false;
			};
			source.set_buffer(Some(buffer));
			source.set_loop(true);
			let wired =
				source.connect_with_audio_node(&gain).is_ok() && gain.connect_with_audio_node(&self.effects).is_ok();
			if !wired || source.start().is_err() {
				return false;
			}
			self.started += 1;
			self.loops.insert(id.to_owned(), (source, gain));
			began = true;
		}
		if let Some((source, gain)) = self.loops.get(id) {
			gain.gain().set_value(loudness);
			source.playback_rate().set_value(speed);
		}
		began
	}

	/// Return the ids of the sounds of an event that have this kind of role.
	fn sounds_of(&self, event: &str, wanted: fn(Role) -> bool) -> Vec<String> {
		self.events
			.get(event)
			.into_iter()
			.flatten()
			.filter(|id| self.sounds.get(*id).is_some_and(|sound| wanted(sound.role)))
			.cloned()
			.collect()
	}

	/// Play the effects and the lines of an event. Return the ids of the sounds that started.
	fn raise(&mut self, event: &str, loudness: f32) -> Vec<String> {
		let now = self.now();
		let mut started = Vec::new();
		for id in self.sounds_of(event, |role| role == Role::Effect) {
			if loudness > 0.01 && self.play_effect(&id, loudness, now) {
				started.push(id);
			}
		}
		for id in self.sounds_of(event, |role| role == Role::Say(Voice::Radio)) {
			if self.say(Voice::Radio, &id, now) {
				started.push(id);
			}
		}
		// Several lines of Betty for one event mean the same, so the event chooses one each time in turn.
		let lines = self.sounds_of(event, |role| role == Role::Say(Voice::Betty));
		if !lines.is_empty() {
			let turn = self.turns.entry(event.to_owned()).or_insert(0);
			let id = &lines[*turn % lines.len()];
			*turn += 1;
			if self.say(Voice::Betty, id, now) {
				started.push(id.clone());
			}
		}
		started
	}

	fn log_event(&self, event: &str, ids: &[String]) {
		if self.log {
			page::log(&format!("SOUND {event} {}", ids.join(",")));
		}
	}

	fn perform(&mut self, cue: Cue) {
		match cue {
			Cue::Raise(event, loudness) => {
				let started = self.raise(event, loudness);
				self.log_event(event, &started);
			}
			Cue::Repeat(event) => {
				let now = self.now();
				if self.repeats.get(event).is_some_and(|until| now < *until) {
					return;
				}
				// A state repeats its line at the pause of the line.
				let pause = self
					.sounds_of(event, |role| matches!(role, Role::Say(_)))
					.iter()
					.filter_map(|id| self.sounds.get(id))
					.map(|sound| sound.cooldown)
					.fold(1.0, f64::max);
				self.repeats.insert(event.to_owned(), now + pause);
				let started = self.raise(event, 1.0);
				self.log_event(event, &started);
			}
			Cue::Hold(event, loudness, speed) => {
				let started: Vec<String> = self
					.sounds_of(event, |role| role == Role::Loop)
					.into_iter()
					.filter(|id| self.hold(id, loudness, speed))
					.collect();
				if !started.is_empty() {
					self.log_event(event, &started);
				}
			}
			Cue::Silence => {
				for (source, _) in std::mem::take(&mut self.loops).into_values() {
					stop(&source);
				}
			}
		}
	}
}

/// Give the cues of a frame to the sound, with the volumes of the settings, each from 0 for silence to 1.
fn perform(cues: Cues, effects: f32, voices: f32) {
	AUDIO.with_borrow_mut(|audio| {
		let Some(audio) = audio else { return };
		audio.effects.gain().set_value(effects);
		audio.voices.gain().set_value(voices);
		for cue in cues.0 {
			audio.perform(cue);
		}
		let now = audio.now();
		audio.pump(now);
	});
}

/// Raise an event outside the match, such as `ui_click`, at a loudness from 0 to 1.
pub fn play(event: &str, loudness: f32) {
	AUDIO.with_borrow_mut(|audio| {
		if let Some(audio) = audio {
			let started = audio.raise(event, loudness);
			audio.log_event(event, &started);
		}
	});
}

/// Return the number of sounds that started since the page loaded.
pub fn started() -> u32 {
	AUDIO.with_borrow(|audio| audio.as_ref().map_or(0, |audio| audio.started))
}

use std::collections::BTreeMap;
use std::f32::consts::TAU;

use glam::{Quat, Vec3};

use crate::{
	bot::{Skill, bot_controls},
	gun::{Bullet, Gun, hit_part},
	missile::{Launcher, Missile, burst_damage},
	plane::{Controls, Part, Plane},
	rng::Rng,
	tuning,
	wire::{Message, PlaneId, PlaneState, Settings},
};

/// Something that happened in a match, for the screen of this machine.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
	/// A shot of a plane that this machine flies hit a plane. The shooter sees it without delay.
	Hit {
		/// The plane that shot.
		by: PlaneId,
		/// The plane that was hit.
		target: PlaneId,
		/// The part that was hit.
		part: Part,
	},
	/// A plane that this machine flies took damage.
	Damage {
		/// The plane that took the damage.
		plane: PlaneId,
		/// The part that took it.
		part: Part,
		/// Whether the part broke off.
		broke: bool,
	},
	/// A plane was destroyed. Every machine sees the kills in the same order.
	Kill {
		/// The plane that was destroyed.
		victim: PlaneId,
		/// The plane that gets the kill.
		killer: Option<PlaneId>,
	},
	/// A missile burst.
	Burst {
		/// The place of the burst, in metres.
		position: Vec3,
	},
}

/// A plane that this machine flies: the plane of its player, or a bot.
#[derive(Clone, Debug)]
struct Local {
	id: PlaneId,
	plane: Plane,
	/// The skill of the bot that flies the plane. The plane of the player has none.
	bot: Option<Skill>,
	gun: Gun,
	launcher: Launcher,
	/// The controls of the last step.
	controls: Controls,
	kills: u16,
	deaths: u16,
	/// The seconds until the plane flies again. A plane that flies has none.
	respawn: Option<f32>,
	/// The last plane that damaged this one, and the seconds since then.
	attacker: Option<(PlaneId, f32)>,
}

impl Local {
	fn state(&self) -> PlaneState {
		let alive = self.respawn.is_none();
		PlaneState {
			position: self.plane.position,
			orientation: self.plane.orientation,
			speed: self.plane.speed,
			health: Part::ALL.map(|part| self.plane.health(part).ceil() as u8),
			alive,
			protected: self.plane.is_protected(),
			firing: self.controls.fire && alive,
			boosting: self.plane.is_boosting(),
			lock: self.launcher.lock().map(|(target, _)| target),
			kills: self.kills,
			deaths: self.deaths,
		}
	}
}

/// A plane that another machine flies, as this machine knows it.
#[derive(Clone, Debug, Default)]
struct Remote {
	shown: Option<PlaneState>,
	gun: Gun,
}

/// One match, as one machine holds it.
///
/// The machine flies its own planes: the plane of its player, and the bots if it hosts the room.
/// It decides their movement and the hits of their shots. It learns every other plane from the
/// states that arrive, and it applies the events of the match in the order of the bus. A solo
/// match is the same with one machine, which receives its own messages at once.
#[derive(Clone, Debug)]
pub struct Match {
	me: u32,
	host: u32,
	settings: Settings,
	rng: Rng,
	steps: u64,
	locals: Vec<Local>,
	remotes: BTreeMap<PlaneId, Remote>,
	bullets: Vec<Bullet>,
	missiles: Vec<Missile>,
	outbox: Vec<Message>,
	events: Vec<Event>,
}

impl Match {
	/// Start the match with the given `seed` and `settings` on the machine of player `me`.
	///
	/// The plane of the player spawns at once. Call [`Match::set_room`] next, so that the match
	/// knows the other members and the host.
	#[must_use]
	pub fn new(me: u32, seed: u32, settings: Settings) -> Self {
		// Each machine draws its own spawn places from the seed of the match.
		let rng = Rng::new(u64::from(seed) << 32 | u64::from(me));
		let mut game = Self {
			me,
			host: me,
			settings,
			rng,
			steps: 0,
			locals: Vec::new(),
			remotes: BTreeMap::new(),
			bullets: Vec::new(),
			missiles: Vec::new(),
			outbox: Vec::new(),
			events: Vec::new(),
		};
		game.spawn(PlaneId::Player(me), None);
		game
	}

	/// Tell the match which players are in the room, and which of them hosts it.
	///
	/// The planes of players that left disappear. The host flies the bots, so a machine that
	/// becomes the host takes them over where they are.
	pub fn set_room(&mut self, members: &[u32], host: u32) {
		self.host = host;
		self.remotes.retain(|id, _| match id {
			PlaneId::Player(number) => members.contains(number),
			PlaneId::Bot(_) => true,
		});
		if host != self.me {
			return;
		}
		for bot in (0..self.settings.bots).map(PlaneId::Bot) {
			if self.flies(bot) {
				continue;
			}
			let skill = Some(self.settings.skill);
			match self.remotes.remove(&bot).and_then(|remote| remote.shown) {
				Some(state) => self.take_over(bot, skill, &state),
				None => self.spawn(bot, skill),
			}
		}
	}

	/// Advance the match by one step of [`tuning::STEP`] seconds.
	///
	/// `now` is the clock of this machine in milliseconds, and `input` holds the controls of its
	/// player. The messages for the other machines wait in [`Match::drain_messages`].
	pub fn step(&mut self, now: f64, input: &Controls) {
		let dt = tuning::STEP;
		self.steps += 1;
		let seen = self.planes();
		self.fly_locals(input, &seen, dt);
		let planes = self.planes();
		self.fire_guns(&planes, dt);
		self.move_bullets(&planes, dt);
		self.launch_missiles(&planes, dt);
		self.move_missiles(&planes, dt);
		if self.steps.is_multiple_of(tuning::STEPS_PER_STATE) {
			// The clock wraps after 49 days, which no match reaches.
			let clock = now as u64 as u32;
			let states = self.locals.iter().map(|local| Message::State {
				plane: local.id,
				clock,
				state: local.state(),
			});
			self.outbox.extend(states);
		}
	}

	/// Apply a message that the bus delivered from the member `sender`.
	///
	/// `now` is the clock of this machine in milliseconds. The machine also receives its own
	/// messages here, in the order in which every machine sees them.
	pub fn receive(&mut self, _now: f64, sender: u32, message: &Message) {
		match *message {
			Message::State { plane, state, .. } => {
				if self.may_fly(sender, plane) && !self.flies(plane) {
					self.remotes.entry(plane).or_default().shown = Some(state);
				}
			}
			Message::Hit {
				target,
				part,
				damage,
				by,
			} => self.take_hit(target, part, damage, by),
			Message::Kill { victim, killer } => {
				if !self.may_fly(sender, victim) {
					return;
				}
				for local in &mut self.locals {
					local.deaths += u16::from(local.id == victim);
					local.kills += u16::from(Some(local.id) == killer);
				}
				self.events.push(Event::Kill { victim, killer });
			}
			Message::Launch {
				shooter,
				missile,
				target,
				position,
				direction,
			} => {
				if self.may_fly(sender, shooter) && !self.flies(shooter) {
					self.missiles
						.push(Missile::new(shooter, missile, target, position, direction, false));
				}
			}
			Message::Burst {
				shooter,
				missile,
				position,
			} => {
				if self.may_fly(sender, shooter) {
					self.missiles
						.retain(|other| other.live || (other.shooter, other.number) != (shooter, missile));
					self.events.push(Event::Burst { position });
				}
			}
			Message::EndMatch | Message::Lobby { .. } | Message::Profile { .. } => {}
		}
	}

	/// Take the messages that this machine must send, in their order.
	pub fn drain_messages(&mut self) -> Vec<Message> {
		std::mem::take(&mut self.outbox)
	}

	/// Take the events since the last call, in their order.
	pub fn drain_events(&mut self) -> Vec<Event> {
		std::mem::take(&mut self.events)
	}

	/// Return the plane of the player of this machine.
	#[must_use]
	pub fn me(&self) -> PlaneId {
		PlaneId::Player(self.me)
	}

	/// Return every plane of the match, as this machine knows it now.
	#[must_use]
	pub fn planes(&self) -> Vec<(PlaneId, PlaneState)> {
		let locals = self.locals.iter().map(|local| (local.id, local.state()));
		let remotes = self
			.remotes
			.iter()
			.filter_map(|(&id, remote)| Some((id, remote.shown?)));
		locals.chain(remotes).collect()
	}

	/// Return the plane with this id, if this machine flies it.
	#[must_use]
	pub fn plane(&self, id: PlaneId) -> Option<&Plane> {
		self.local(id).map(|local| &local.plane)
	}

	/// Return the plane with this id for a change, if this machine flies it.
	///
	/// This is the hook of tests and playtests, which put a plane into a chosen state.
	pub fn plane_mut(&mut self, id: PlaneId) -> Option<&mut Plane> {
		self.locals
			.iter_mut()
			.find(|local| local.id == id)
			.map(|local| &mut local.plane)
	}

	/// Return the seconds until the plane with this id flies again, if this machine flies it and it is down.
	#[must_use]
	pub fn respawn_in(&self, id: PlaneId) -> Option<f32> {
		self.local(id)?.respawn
	}

	/// Return the target of the missile lock of the plane with this id, and how far the lock
	/// closed, from 0 to 1, if this machine flies the plane.
	#[must_use]
	pub fn lock(&self, id: PlaneId) -> Option<(PlaneId, f32)> {
		self.local(id)?.launcher.lock()
	}

	/// Return the number of missiles that the plane with this id can launch, if this machine flies it.
	#[must_use]
	pub fn missiles_ready(&self, id: PlaneId) -> Option<u8> {
		Some(self.local(id)?.launcher.ready())
	}

	/// Return the bullets in flight.
	#[must_use]
	pub fn bullets(&self) -> &[Bullet] {
		&self.bullets
	}

	/// Return the missiles in flight.
	#[must_use]
	pub fn missiles(&self) -> &[Missile] {
		&self.missiles
	}

	fn local(&self, id: PlaneId) -> Option<&Local> {
		self.locals.iter().find(|local| local.id == id)
	}

	fn flies(&self, id: PlaneId) -> bool {
		self.local(id).is_some()
	}

	/// Return whether the member `sender` is the one that flies `plane`.
	fn may_fly(&self, sender: u32, plane: PlaneId) -> bool {
		match plane {
			PlaneId::Player(number) => number == sender,
			PlaneId::Bot(_) => sender == self.host,
		}
	}

	fn spawn(&mut self, id: PlaneId, bot: Option<Skill>) {
		let plane = self.new_plane();
		let controls = Controls {
			aim: plane.forward(),
			throttle: 0.5,
			boost: false,
			fire: false,
			launch: false,
		};
		self.locals.push(Local {
			id,
			plane,
			bot,
			gun: Gun::default(),
			launcher: Launcher::default(),
			controls,
			kills: 0,
			deaths: 0,
			respawn: None,
			attacker: None,
		});
	}

	/// Continue a bot that the last host flew, from the state that this machine saw last.
	fn take_over(&mut self, id: PlaneId, bot: Option<Skill>, state: &PlaneState) {
		self.spawn(id, bot);
		let local = self.locals.last_mut().expect("the spawn added a plane");
		local.kills = state.kills;
		local.deaths = state.deaths;
		if state.alive {
			let health = Part::ALL.map(|part| f32::from(state.health[part as usize]));
			local.plane = Plane::restored(state.position, state.orientation, state.speed, health);
		}
	}

	/// Return a whole, protected plane at a random place around the centre, with its nose toward the centre.
	fn new_plane(&mut self) -> Plane {
		let angle = self.rng.range(0.0, TAU);
		let radius = self.rng.range(tuning::SPAWN_MIN_RADIUS, tuning::SPAWN_MAX_RADIUS);
		let height = self.rng.range(tuning::SPAWN_MIN_HEIGHT, tuning::SPAWN_MAX_HEIGHT);
		let position = Vec3::new(radius * angle.sin(), height, radius * angle.cos());
		let mut plane = Plane::new(position, Quat::from_rotation_y(angle));
		plane.protect(tuning::SPAWN_PROTECTION);
		plane
	}

	fn fly_locals(&mut self, input: &Controls, seen: &[(PlaneId, PlaneState)], dt: f32) {
		for index in 0..self.locals.len() {
			if let Some(left) = self.locals[index].respawn {
				if left > dt {
					self.locals[index].respawn = Some(left - dt);
				} else {
					let plane = self.new_plane();
					let local = &mut self.locals[index];
					(local.plane, local.respawn, local.attacker) = (plane, None, None);
				}
				continue;
			}
			let local = &mut self.locals[index];
			let controls = match local.bot {
				Some(skill) => bot_controls(local.id, &local.state(), seen, skill),
				None => *input,
			};
			local.controls = controls;
			local.plane.step(&local.controls, dt);
			local.attacker = local.attacker.map(|(by, since)| (by, since + dt));
			if local.plane.is_destroyed() {
				// The plane hit the ground. Its last attacker gets the kill, if the hit was recent
				// or cost the plane a part.
				let crippled = Part::ALL
					.into_iter()
					.any(|part| part != Part::Fuselage && local.plane.is_broken(part));
				let killer = local
					.attacker
					.filter(|&(_, since)| crippled || since < tuning::KILL_CREDIT_SECONDS)
					.map(|(by, _)| by);
				Self::go_down(local, killer, &mut self.outbox);
			}
		}
	}

	/// Take a destroyed plane out of the fight, and tell every machine.
	fn go_down(local: &mut Local, killer: Option<PlaneId>, outbox: &mut Vec<Message>) {
		local.respawn = Some(tuning::RESPAWN_SECONDS);
		outbox.push(Message::Kill {
			victim: local.id,
			killer,
		});
	}

	fn fire_guns(&mut self, planes: &[(PlaneId, PlaneState)], dt: f32) {
		for (id, state) in planes {
			let (gun, live) = match self.locals.iter_mut().find(|local| local.id == *id) {
				Some(local) => (&mut local.gun, true),
				None => match self.remotes.get_mut(id) {
					Some(remote) => (&mut remote.gun, false),
					None => continue,
				},
			};
			self.bullets.extend(gun.step(*id, state, live, &mut self.rng, dt));
		}
	}

	fn move_bullets(&mut self, planes: &[(PlaneId, PlaneState)], dt: f32) {
		let (outbox, events) = (&mut self.outbox, &mut self.events);
		self.bullets.retain_mut(|bullet| {
			let from = bullet.step(dt);
			if !bullet.live {
				return !bullet.is_spent();
			}
			let targets = planes
				.iter()
				.filter(|(id, state)| *id != bullet.shooter && state.can_be_hit());
			let hit = targets
				.filter_map(|(id, state)| Some((*id, hit_part(state, from, bullet.position)?)))
				.next();
			if let Some((target, part)) = hit {
				outbox.push(Message::Hit {
					target,
					part,
					damage: tuning::BULLET_DAMAGE,
					by: bullet.shooter,
				});
				events.push(Event::Hit {
					by: bullet.shooter,
					target,
					part,
				});
			}
			hit.is_none() && !bullet.is_spent()
		});
	}

	fn launch_missiles(&mut self, planes: &[(PlaneId, PlaneState)], dt: f32) {
		for local in &mut self.locals {
			let state = local.state();
			if let Some(missile) = local.launcher.step(local.id, &state, planes, local.controls.launch, dt) {
				self.outbox.push(Message::Launch {
					shooter: missile.shooter,
					missile: missile.number,
					target: missile.target,
					position: missile.position,
					direction: missile.direction,
				});
				self.missiles.push(missile);
			}
		}
	}

	fn move_missiles(&mut self, planes: &[(PlaneId, PlaneState)], dt: f32) {
		let (outbox, events) = (&mut self.outbox, &mut self.events);
		self.missiles.retain_mut(|missile| {
			let fused = missile.step(planes, dt);
			if !(missile.live && fused) {
				return !missile.is_spent();
			}
			// This machine flies the missile, so it decides the damage of the burst.
			let (shooter, position) = (missile.shooter, missile.position);
			outbox.push(Message::Burst {
				shooter,
				missile: missile.number,
				position,
			});
			for (target, state) in planes.iter().filter(|(id, state)| *id != shooter && state.can_be_hit()) {
				for (part, damage) in burst_damage(position, state) {
					outbox.push(Message::Hit {
						target: *target,
						part,
						damage,
						by: shooter,
					});
					events.push(Event::Hit {
						by: shooter,
						target: *target,
						part,
					});
				}
			}
			false
		});
	}

	/// Apply a hit to its target, if this machine flies the target.
	fn take_hit(&mut self, target: PlaneId, part: Part, damage: f32, by: PlaneId) {
		let Some(local) = self
			.locals
			.iter_mut()
			.find(|local| local.id == target && local.respawn.is_none())
		else {
			return;
		};
		if local.plane.is_protected() || local.plane.is_broken(part) || !damage.is_finite() || damage <= 0.0 {
			return;
		}
		let broke = local.plane.damage(part, damage);
		local.attacker = Some((by, 0.0));
		self.events.push(Event::Damage {
			plane: target,
			part,
			broke,
		});
		if local.plane.is_destroyed() {
			Self::go_down(local, Some(by), &mut self.outbox);
		}
	}
}

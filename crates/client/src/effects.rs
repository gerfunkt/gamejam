//! The effects of the fight, built from soft puffs of fire and smoke: the burst of a missile, the
//! end of a plane, the fire and the trail of a missile, the flares, the smoke of a damaged plane,
//! and the flame and the trail of a plane that boosts. While the plane of the player boosts, thin
//! streaks rush past the camera.
//!
//! The effects read the match and never change it.

use std::collections::{HashMap, VecDeque};

use bevy::{light::NotShadowCaster, prelude::*};
use sim::{Event, Part, Session, tuning, wire::PlaneId};

use crate::{
	flow::{Flow, Game},
	puff::{self, Billboard, PuffAssets},
	view::Drawn,
};

/// The most puffs that show at once. A new puff then takes the place of the oldest one.
const PUFF_LIMIT: usize = 1500;
/// The number of materials along the life of a puff. A puff changes its colour in this many steps.
const STEPS: usize = 16;
/// The number of steps in which a puff fades when the camera comes near.
const FADES: usize = 3;
/// The distance from the camera, as a multiple of the size of a puff, at which a puff starts to fade.
const FADE_START: f32 = 3.0;
/// The distance from the camera, as a multiple of the size of a puff, at which a puff is gone.
const FADE_END: f32 = 0.8;
/// How fast a puff loses its speed in the air, as a part for each second.
const DRAG: f32 = 1.6;
/// How fast smoke rises, in m/s.
const RISE: f32 = 2.5;
/// The most puffs that one trail adds in one frame.
const TRAIL_BURST: usize = 8;
/// A trail whose source moved further than this in one frame starts again, in metres.
const TRAIL_JUMP: f32 = 80.0;
/// The radius of the ball of fire at the end of a plane, in metres.
const KILL_RADIUS: f32 = 20.0;
/// The number of puffs in the ball of fire of a burst, and of the end of a plane.
const BURST_PUFFS: usize = 14;
const KILL_PUFFS: usize = 24;
/// How long a ball of fire and its smoke last: from, to, in seconds.
const BLAST_LIFE: (f32, f32) = (3.0, 5.0);
/// The size of the flame at the tail of a missile, in metres, and its distance behind the centre.
const FLAME_SIZE: f32 = 5.0;
const FLAME_BEHIND: f32 = 2.2;
/// The size of the glow of a flare, in metres.
const FLARE_SIZE: f32 = 9.0;
/// The place of the flame of a boost, behind the tail, in the frame of the plane, in metres.
const EXHAUST: Vec3 = Vec3::new(0.0, 0.2, 4.4);
/// The size of the flame of a boost, in metres.
const EXHAUST_SIZE: f32 = 7.0;
/// The number of speed streaks around the camera while the player boosts.
const STREAKS: usize = 36;
/// How far ahead of the camera a streak starts: from, to, in metres.
const STREAK_AHEAD: (f32, f32) = (40.0, 140.0);
/// How far from the line of flight a streak passes: from, to, in metres.
const STREAK_ASIDE: (f32, f32) = (3.0, 14.0);
/// The length of a streak, as the distance that the plane flies in this time, in seconds.
const STREAK_TIME: f32 = 0.07;
/// The thickness of a streak, in metres.
const STREAK_WIDTH: f32 = 0.05;

/// The kinds of puff, each with its own life, size, and colours.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Kind {
	/// The flash at the heart of a burst.
	Flash,
	/// A ball of fire that turns into dark smoke.
	Blast,
	/// The trail of a missile: a little fire, then light grey smoke.
	Missile,
	/// The trail of a flare: white smoke.
	Flare,
	/// The grey smoke of a plane with a dead engine.
	Engine,
	/// The thick black smoke of a plane that falls after it lost a wing.
	Wing,
	/// The dense, short trail of a plane that boosts: hot at first, then white.
	Boost,
}

impl Kind {
	const ALL: [Kind; 7] = [
		Kind::Flash,
		Kind::Blast,
		Kind::Missile,
		Kind::Flare,
		Kind::Engine,
		Kind::Wing,
		Kind::Boost,
	];

	/// Return the life of a trail puff in seconds, its size at its start and at its end in metres,
	/// and the distance between two puffs of its trail in metres.
	///
	/// # Panics
	///
	/// Panics for the kinds of a ball of fire, which lay no trail.
	fn trail(self) -> (f32, f32, f32, f32) {
		match self {
			Kind::Missile => (2.0, 2.5, 10.0, 4.0),
			Kind::Flare => (1.2, 2.0, 7.0, 3.0),
			Kind::Engine => (2.4, 3.0, 13.0, 6.0),
			Kind::Wing => (3.2, 5.0, 20.0, 4.0),
			Kind::Boost => (0.7, 1.6, 4.5, 1.5),
			Kind::Flash | Kind::Blast => unreachable!("only a missile, a flare, and a damaged plane lay a trail"),
		}
	}

	/// Return the colour of a puff at a point of its life from 0 to 1, with its cover as the alpha.
	fn colour(self, along: f32) -> LinearRgba {
		// The part of the life in fire, the grey of the smoke, and the cover of the smoke.
		let (fire, grey, cover) = match self {
			Kind::Flash => return LinearRgba::rgb(9.0, 6.0, 3.0) * (1.0 - along).powi(2),
			Kind::Blast => (0.16, 0.12, 0.9),
			Kind::Missile => (0.06, 0.6, 0.55),
			Kind::Flare => (0.12, 0.8, 0.45),
			Kind::Engine => (0.0, 0.22, 0.5),
			Kind::Wing => (0.04, 0.07, 0.75),
			Kind::Boost => (0.2, 0.85, 0.65),
		};
		let hot = LinearRgba::rgb(8.0, 4.5, 1.4);
		let embers = LinearRgba::rgb(0.9, 0.28, 0.05);
		let smoke = LinearRgba::rgb(grey, grey, grey);
		// The fire cools from yellow to red, and then the smoke takes its place.
		let colour = if along < fire {
			let cool = along / fire;
			hot * (1.0 - cool) + embers * cool
		} else {
			let settle = ((along - fire) / fire.max(0.05)).min(1.0);
			embers * (1.0 - settle) + smoke * settle
		};
		// The smoke thins out over the second half of its life.
		let thin = (1.0 - ((along - 0.4) / 0.6).max(0.0)).powf(1.3);
		colour.with_alpha(cover * thin)
	}
}

/// The materials of every kind of puff, for each step of its fade near the camera and each step of its life.
#[derive(Resource)]
struct Looks(HashMap<Kind, [[Handle<StandardMaterial>; STEPS]; FADES]>);

impl Looks {
	/// Return the material of a puff at a point of its life from 0 to 1, at a step of its fade from 1 to [`FADES`].
	fn get(&self, kind: Kind, along: f32, fade: usize) -> Handle<StandardMaterial> {
		let step = ((along * STEPS as f32) as usize).min(STEPS - 1);
		self.0[&kind][fade - 1][step].clone()
	}
}

/// The materials of the glows at the tail of a missile, of a flare, and of a plane that boosts.
#[derive(Resource)]
struct Glows {
	flame: Handle<StandardMaterial>,
	flare: Handle<StandardMaterial>,
	exhaust: Handle<StandardMaterial>,
}

/// The mesh and the material of the speed streaks.
#[derive(Resource)]
struct StreakLook {
	line: Handle<Mesh>,
	glow: Handle<StandardMaterial>,
}

/// A speed streak, which stays at its place in the world while the camera rushes past it.
#[derive(Component)]
struct Streak;

/// One puff of fire or smoke.
#[derive(Component)]
struct Puff {
	kind: Kind,
	/// The age and the life, in seconds.
	age: f32,
	life: f32,
	/// The side of its square at its start and at its end, in metres.
	size: (f32, f32),
	/// The velocity, in m/s.
	velocity: Vec3,
}

/// The puffs that show, from the oldest to the newest.
#[derive(Resource, Default)]
struct Puffs(VecDeque<Entity>);

/// A glow at the tail of a missile or of a flare. The glows of one frame are reused in the next.
#[derive(Component)]
struct Glow;

/// A thing that leaves a trail: a missile, a flare, or a damaged plane.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Source(Kind, PlaneId, u8);

/// The place of the last puff of each trail.
#[derive(Resource, Default)]
struct Trails(HashMap<Source, Vec3>);

pub fn plugin(app: &mut App) {
	app.init_resource::<Puffs>()
		.init_resource::<Trails>()
		.add_systems(Startup, make_looks)
		.add_systems(
			Update,
			(age_puffs, start_blasts, lay_trails, show_glows, show_streaks)
				.chain()
				.after(Flow)
				.in_set(Drawn),
		);
}

fn make_looks(
	mut commands: Commands,
	assets: Res<PuffAssets>,
	mut meshes: ResMut<Assets<Mesh>>,
	mut materials: ResMut<Assets<StandardMaterial>>,
) {
	let looks = Kind::ALL.map(|kind| {
		let steps = std::array::from_fn(|fade| {
			let thin = (fade + 1) as f32 / FADES as f32;
			std::array::from_fn(|step| {
				let along = (step as f32 + 0.5) / STEPS as f32;
				let colour = kind.colour(along);
				let (texture, alpha_mode, colour) = match kind {
					Kind::Flash => (assets.glow.clone(), AlphaMode::Add, colour * thin),
					_ => (
						assets.cloud.clone(),
						AlphaMode::Blend,
						colour.with_alpha(colour.alpha * thin),
					),
				};
				materials.add(StandardMaterial {
					base_color: colour.into(),
					base_color_texture: Some(texture),
					unlit: true,
					alpha_mode,
					cull_mode: None,
					..default()
				})
			})
		});
		(kind, steps)
	});
	commands.insert_resource(Looks(looks.into_iter().collect()));
	let glow = |colour: LinearRgba| StandardMaterial {
		base_color: colour.into(),
		base_color_texture: Some(assets.glow.clone()),
		unlit: true,
		alpha_mode: AlphaMode::Add,
		cull_mode: None,
		..default()
	};
	commands.insert_resource(Glows {
		flame: materials.add(glow(LinearRgba::rgb(6.0, 3.0, 0.8))),
		flare: materials.add(glow(LinearRgba::rgb(9.0, 8.0, 6.0))),
		exhaust: materials.add(glow(LinearRgba::rgb(7.0, 5.5, 3.5))),
	});
	commands.insert_resource(StreakLook {
		line: meshes.add(Cuboid::new(STREAK_WIDTH, STREAK_WIDTH, 1.0)),
		glow: materials.add(StandardMaterial {
			base_color: LinearRgba::rgb(1.6, 1.6, 1.7).into(),
			unlit: true,
			alpha_mode: AlphaMode::Add,
			..default()
		}),
	});
}

/// Start a puff, in the place of the oldest puff when the limit is reached.
fn start_puff(commands: &mut Commands, puffs: &mut Puffs, assets: &PuffAssets, looks: &Looks, place: Vec3, puff: Puff) {
	let look = (
		MeshMaterial3d(looks.get(puff.kind, 0.0, FADES)),
		Transform::from_translation(place).with_scale(Vec3::splat(puff.size.0)),
		Visibility::Inherited,
		puff,
	);
	if puffs.0.len() >= PUFF_LIMIT
		&& let Some(oldest) = puffs.0.pop_front()
	{
		commands.entity(oldest).insert(look);
		puffs.0.push_back(oldest);
		return;
	}
	let entity = commands
		.spawn((Billboard, Mesh3d(assets.square.clone()), NotShadowCaster, look))
		.id();
	puffs.0.push_back(entity);
}

/// Return a pseudo-random number from 0 to 1. The effects need no agreement between machines.
fn random(counter: &mut u32) -> f32 {
	*counter = counter.wrapping_add(1);
	puff::random(*counter)
}

/// Return a pseudo-random direction.
fn random_direction(counter: &mut u32) -> Vec3 {
	let (turn, rise) = (random(counter) * std::f32::consts::TAU, random(counter) * 2.0 - 1.0);
	let across = (1.0 - rise * rise).sqrt();
	Vec3::new(turn.cos() * across, rise, turn.sin() * across)
}

/// Move, grow, and colour each puff, and remove each puff at the end of its life.
fn age_puffs(
	mut commands: Commands,
	time: Res<Time>,
	looks: Res<Looks>,
	mut puffs: ResMut<Puffs>,
	camera: Single<&Transform, (With<Camera3d>, Without<Puff>)>,
	mut shown: Query<(
		Entity,
		&mut Puff,
		&mut Transform,
		&mut MeshMaterial3d<StandardMaterial>,
		&mut Visibility,
	)>,
) {
	let delta = time.delta_secs();
	let slow = (-DRAG * delta).exp();
	let mut ended = false;
	for (entity, mut puff, mut transform, mut material, mut visibility) in &mut shown {
		puff.age += delta;
		if puff.age >= puff.life {
			commands.entity(entity).despawn();
			ended = true;
			continue;
		}
		puff.velocity = puff.velocity * slow + Vec3::Y * RISE * (1.0 - slow);
		let along = puff.age / puff.life;
		// A puff grows fast at first, and then slower.
		let grown = 1.0 - (1.0 - along).powi(3);
		transform.translation += puff.velocity * delta;
		let size = puff.size.0 + (puff.size.1 - puff.size.0) * grown;
		transform.scale = Vec3::splat(size);
		// A puff near the camera thins out, so that smoke never hides the picture for long.
		let near = transform.translation.distance(camera.translation) / size;
		let fade = ((near - FADE_END) / (FADE_START - FADE_END) * FADES as f32)
			.ceil()
			.clamp(0.0, FADES as f32) as usize;
		let hidden = fade == 0;
		if (*visibility == Visibility::Hidden) != hidden {
			*visibility = if hidden {
				Visibility::Hidden
			} else {
				Visibility::Inherited
			};
		}
		if hidden {
			continue;
		}
		let look = looks.get(puff.kind, along, fade);
		if material.0 != look {
			material.0 = look;
		}
	}
	if ended {
		puffs
			.0
			.retain(|entity| shown.get(*entity).is_ok_and(|(_, puff, ..)| puff.age < puff.life));
	}
}

/// Start a ball of fire for each burst of a missile and for each end of a plane.
fn start_blasts(
	mut commands: Commands,
	game: Res<Game>,
	assets: Res<PuffAssets>,
	looks: Res<Looks>,
	mut puffs: ResMut<Puffs>,
	mut counter: Local<u32>,
) {
	let planes = game
		.session
		.as_ref()
		.and_then(Session::game)
		.map(sim::Match::planes)
		.unwrap_or_default();
	for event in &game.events {
		let (place, radius, count) = match event {
			Event::Burst { position } => (Some(*position), tuning::BURST_RADIUS, BURST_PUFFS),
			Event::Kill { victim, .. } => (
				planes
					.iter()
					.find(|(id, _)| id == victim)
					.map(|(_, state)| state.position),
				KILL_RADIUS,
				KILL_PUFFS,
			),
			_ => continue,
		};
		let Some(place) = place else { continue };
		let flash = Puff {
			kind: Kind::Flash,
			age: 0.0,
			life: 0.35,
			size: (radius * 2.0, radius * 5.0),
			velocity: Vec3::ZERO,
		};
		start_puff(&mut commands, &mut puffs, &assets, &looks, place, flash);
		for _ in 0..count {
			let out = random_direction(&mut counter);
			let reach = random(&mut counter);
			let blast = Puff {
				kind: Kind::Blast,
				age: 0.0,
				life: BLAST_LIFE.0 + (BLAST_LIFE.1 - BLAST_LIFE.0) * random(&mut counter),
				size: (radius * 0.9, radius * (2.0 + reach)),
				velocity: out * radius * (1.0 + 2.0 * reach),
			};
			start_puff(
				&mut commands,
				&mut puffs,
				&assets,
				&looks,
				place + out * radius * 0.5 * reach,
				blast,
			);
		}
	}
}

/// Lay the trails of smoke behind the missiles, the flares, and the damaged planes.
fn lay_trails(
	mut commands: Commands,
	game: Res<Game>,
	assets: Res<PuffAssets>,
	looks: Res<Looks>,
	mut puffs: ResMut<Puffs>,
	mut trails: ResMut<Trails>,
	mut counter: Local<u32>,
) {
	let Some(running) = game.session.as_ref().and_then(Session::game) else {
		trails.0.clear();
		return;
	};
	let mut sources = Vec::new();
	for missile in running.missiles() {
		let tail = missile.position - missile.direction * FLAME_BEHIND;
		sources.push((Source(Kind::Missile, missile.shooter, missile.number), tail));
	}
	let mut flares: HashMap<PlaneId, u8> = HashMap::new();
	for flare in running.flares() {
		let number = flares.entry(flare.owner).or_default();
		sources.push((Source(Kind::Flare, flare.owner, *number), flare.position));
		*number += 1;
	}
	for (id, state) in running.planes().into_iter().filter(|(_, state)| state.alive) {
		if state.boosting {
			sources.push((Source(Kind::Boost, id, 0), state.position + state.orientation * EXHAUST));
		}
		if state.is_broken(Part::Engine) {
			sources.push((
				Source(Kind::Engine, id, 0),
				state.position + state.orientation * Part::Engine.centre(),
			));
		}
		for wing in [Part::LeftWing, Part::RightWing]
			.into_iter()
			.filter(|&wing| state.is_broken(wing))
		{
			sources.push((
				Source(Kind::Wing, id, wing as u8),
				state.position + state.orientation * wing.centre(),
			));
		}
	}
	let mut laid = HashMap::new();
	for (source, place) in sources {
		let Source(kind, ..) = source;
		let (life, start, end, spacing) = kind.trail();
		let last = trails
			.0
			.get(&source)
			.copied()
			.filter(|last| last.distance(place) < TRAIL_JUMP);
		let mut lay = |at: Vec3| {
			let puff = Puff {
				kind,
				age: 0.0,
				life: life * (0.8 + 0.4 * random(&mut counter)),
				size: (start, end),
				velocity: random_direction(&mut counter) * 2.0,
			};
			start_puff(&mut commands, &mut puffs, &assets, &looks, at, puff);
		};
		// The puffs stand at even distances along the way since the last puff.
		let at = match last {
			Some(last) => {
				let count = ((last.distance(place) / spacing) as usize).min(TRAIL_BURST);
				let step = (place - last).normalize_or_zero() * spacing;
				for index in 1..=count {
					lay(last + step * index as f32);
				}
				// A trail that fell behind its source catches up, so that it never lays a long line in one frame.
				if count == TRAIL_BURST {
					place
				} else {
					last + step * count as f32
				}
			}
			None => {
				lay(place);
				place
			}
		};
		laid.insert(source, at);
	}
	trails.0 = laid;
}

/// Show a glow at the tail of each missile, on each flare, and behind each plane that boosts.
fn show_glows(
	mut commands: Commands,
	game: Res<Game>,
	assets: Res<PuffAssets>,
	glows: Res<Glows>,
	mut shown: Query<(&mut Transform, &mut MeshMaterial3d<StandardMaterial>, &mut Visibility), With<Glow>>,
) {
	let running = game.session.as_ref().and_then(Session::game);
	let missiles = running.map(sim::Match::missiles).unwrap_or_default();
	let flares = running.map(sim::Match::flares).unwrap_or_default();
	let planes = running.map(sim::Match::planes).unwrap_or_default();
	let boosting = planes
		.iter()
		.filter(|(_, state)| state.alive && state.boosting)
		.map(|(_, state)| {
			(
				state.position + state.orientation * EXHAUST,
				EXHAUST_SIZE,
				&glows.exhaust,
			)
		});
	let wanted = missiles
		.iter()
		.map(|missile| {
			(
				missile.position - missile.direction * FLAME_BEHIND,
				FLAME_SIZE,
				&glows.flame,
			)
		})
		.chain(flares.iter().map(|flare| (flare.position, FLARE_SIZE, &glows.flare)))
		.chain(boosting);
	let mut pool = shown.iter_mut();
	for (place, size, look) in wanted {
		let transform = Transform::from_translation(place).with_scale(Vec3::splat(size));
		match pool.next() {
			Some((mut shown, mut material, mut visibility)) => {
				(shown.translation, shown.scale) = (transform.translation, transform.scale);
				if material.0 != *look {
					material.0 = look.clone();
				}
				*visibility = Visibility::Inherited;
			}
			None => {
				commands.spawn((
					Glow,
					Billboard,
					Mesh3d(assets.square.clone()),
					MeshMaterial3d(look.clone()),
					transform,
					NotShadowCaster,
				));
			}
		}
	}
	for (_, _, mut visibility) in pool {
		*visibility = Visibility::Hidden;
	}
}

/// Show thin streaks that rush past the camera while the plane of the player boosts.
fn show_streaks(
	mut commands: Commands,
	game: Res<Game>,
	look: Res<StreakLook>,
	camera: Single<&Transform, (With<Camera3d>, Without<Streak>)>,
	mut streaks: Query<(&mut Transform, &mut Visibility), With<Streak>>,
	mut counter: Local<u32>,
) {
	let running = game.session.as_ref().and_then(Session::game);
	let me = running.and_then(|running| {
		running
			.planes()
			.into_iter()
			.find_map(|(id, state)| (id == running.me()).then_some(state))
	});
	let Some(plane) = me.filter(|plane| plane.alive && plane.boosting) else {
		for (_, mut visibility) in &mut streaks {
			*visibility = Visibility::Hidden;
		}
		return;
	};
	let (eye, ahead) = (camera.translation, plane.forward());
	let length = plane.speed * STREAK_TIME;
	let place_ahead = |counter: &mut u32| {
		let aside = ahead.any_orthonormal_vector();
		let around = Quat::from_axis_angle(ahead, random(counter) * std::f32::consts::TAU) * aside;
		let reach = STREAK_AHEAD.0 + (STREAK_AHEAD.1 - STREAK_AHEAD.0) * random(counter);
		let off = STREAK_ASIDE.0 + (STREAK_ASIDE.1 - STREAK_ASIDE.0) * random(counter);
		eye + ahead * reach + around * off
	};
	let mut count = 0;
	for (mut transform, mut visibility) in &mut streaks {
		count += 1;
		// A streak that the camera passed, or that a turn left far aside, starts again ahead.
		let from_eye = transform.translation - eye;
		if *visibility == Visibility::Hidden
			|| from_eye.dot(ahead) < -length
			|| from_eye.length() > STREAK_AHEAD.1 * 1.2
		{
			transform.translation = place_ahead(&mut counter);
			*visibility = Visibility::Inherited;
		}
		*transform = Transform::from_translation(transform.translation)
			.looking_to(ahead, Vec3::Y)
			.with_scale(Vec3::new(1.0, 1.0, length));
	}
	for _ in count..STREAKS {
		let place = place_ahead(&mut counter);
		commands.spawn((
			Streak,
			Mesh3d(look.line.clone()),
			MeshMaterial3d(look.glow.clone()),
			Transform::from_translation(place)
				.looking_to(ahead, Vec3::Y)
				.with_scale(Vec3::new(1.0, 1.0, length)),
			NotShadowCaster,
		));
	}
}

//! The picture of the sky: the ground, the planes, the tracers, the missiles, and the camera.
//!
//! The view reads the match and never changes it.

use std::{
	cell::RefCell,
	collections::{HashMap, HashSet},
};

use bevy::{
	asset::RenderAssetUsages,
	ecs::system::SystemParam,
	gltf::{Gltf, GltfMaterial, GltfMesh},
	mesh::Indices,
	prelude::*,
	render::render_resource::PrimitiveTopology,
};
use serde::Deserialize;
use sim::{
	Part, Session, arena, tuning,
	wire::{PlaneId, PlaneState},
};

use crate::{
	flow::{Flow, Game},
	page,
	propeller::Blur,
};

/// The place of the camera behind and above the plane, in the level frame of the plane.
const CHASE_OFFSET: Vec3 = Vec3::new(0.0, 5.0, 21.0);
/// How fast the camera closes the distance to its place, as a part for each second.
const CHASE_PACE: f32 = 12.0;
/// The vertical angle of view, in radians.
const FIELD_OF_VIEW: f32 = 1.05;
/// The sine of the angle between the nose and the vertical below which the camera keeps its side.
const STEEP_START: f32 = 0.12;
/// The sine of the angle between the nose and the vertical above which the horizon of the camera is level.
const STEEP_END: f32 = 0.35;
/// The time of a frame above which the device draws the sky too slowly for a menu, in seconds.
const SLOW_FRAME: f32 = 0.045;
/// The slow frames in a row after which the picture behind the menus stops. A hitch at the start is shorter.
const SLOW_FRAMES: u32 = 40;
/// The colours of the power-ups, in the order of their kinds: speed, weapons, and repair.
///
/// The repair is a light green that is nearly white, because a plain green is lost against the ground.
const POWER_COLOURS: [Color; 3] = [
	Color::srgb(0.3, 0.8, 1.0),
	Color::srgb(1.0, 0.6, 0.2),
	Color::srgb(0.7, 1.0, 0.75),
];
/// The radius and the height of the pillar of light through a power-up, in metres.
const PILLAR_RADIUS: f32 = 2.2;
const PILLAR_HEIGHT: f32 = 420.0;
/// How far the icon of a power-up swells and shrinks, as a part of its size.
const POWER_UP_SWELL: f32 = 0.14;

/// The meshes and the materials that the planes and the shots share.
#[derive(Resource)]
struct Models {
	parts: [Handle<Mesh>; Part::ALL.len()],
	engine: Handle<StandardMaterial>,
	tracer: Handle<Mesh>,
	tracer_glow: Handle<StandardMaterial>,
	/// The body, the nose, and one pair of fins of a missile.
	missile: [Handle<Mesh>; 3],
	missile_paint: Handle<StandardMaterial>,
	missile_trim: Handle<StandardMaterial>,
	/// The ring of a power-up, the sign in it for each kind, and the pillar of light that shows it from afar.
	ring: Handle<Mesh>,
	signs: [Handle<Mesh>; 3],
	power_glow: [Handle<StandardMaterial>; 3],
	pillar: Handle<Mesh>,
	pillar_glow: [Handle<StandardMaterial>; 3],
}

/// One entry of the list of plane models that the build writes beside the page.
#[derive(Deserialize)]
struct ModelEntry {
	/// The file of the model, in the directory of the plane models.
	file: String,
	/// The kills from which a pilot flies this model.
	kills: u16,
	/// The disc of the propeller, if the model has one. The build took its blades out.
	propeller: Option<Propeller>,
}

/// The place and the reach of a propeller, in the frame of its plane, in metres.
#[derive(Clone, Copy, Deserialize)]
struct Propeller {
	centre: Vec3,
	radius: f32,
}

/// The mesh of each part of a plane model, in the order of [`Part::ALL`], and its texture.
type LoadedModel = ([Handle<Mesh>; Part::ALL.len()], Option<Handle<Image>>);

/// A plane model of the build. The build cut it into one mesh for each part of the rules.
struct PlaneModel {
	kills: u16,
	propeller: Option<Propeller>,
	file: Handle<Gltf>,
	/// The meshes and the texture, once the file is loaded.
	parts: Option<LoadedModel>,
}

/// The plane models, in the order of their kills. Without a model, a plane shows as boxes.
#[derive(Resource, Default)]
struct PlaneModels(Vec<PlaneModel>);

impl PlaneModels {
	/// Return the place in the list of the model that a pilot with these kills flies, if it is loaded.
	fn for_kills(&self, kills: u16) -> Option<usize> {
		self.0
			.iter()
			.rposition(|model| model.kills <= kills && model.parts.is_some())
	}
}

thread_local! {
	/// The text of the list of plane models, from its arrival until the view reads it.
	static MODEL_LIST: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The root of the picture of one plane, with the place of its model in the list of models.
#[derive(Component)]
struct PlaneView(PlaneId, Option<usize>);

/// The box of one part of a plane.
#[derive(Component)]
struct PartView(PlaneId, Part);

#[derive(Component)]
struct Tracer;

#[derive(Component)]
struct MissileView;

/// The icon of the power-up at this place of the list of the rules.
#[derive(Component)]
struct PowerUpView(usize);

pub fn plugin(app: &mut App) {
	app.init_resource::<PlaneModels>()
		.add_systems(Startup, (make_models, make_arena, make_camera))
		.add_systems(Update, load_plane_models.before(Drawn))
		.add_systems(
			Update,
			(show_planes, show_tracers, show_missiles, show_power_ups, follow)
				.chain()
				.after(Flow)
				.in_set(Drawn),
		);
}

/// The systems that draw the match and place the camera. The display of the page runs after them.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Drawn;

fn make_models(
	mut commands: Commands,
	mut meshes: ResMut<Assets<Mesh>>,
	mut materials: ResMut<Assets<StandardMaterial>>,
) {
	let glow = |colour: Color, light: LinearRgba| StandardMaterial {
		base_color: colour,
		emissive: light,
		unlit: true,
		..default()
	};
	commands.insert_resource(Models {
		// The box of each part comes from the rules, so the picture matches what a shot can hit.
		parts: Part::ALL.map(|part| meshes.add(Cuboid::from_size(part.half_extents() * 2.0))),
		engine: materials.add(Color::srgb(0.25, 0.25, 0.28)),
		tracer: meshes.add(Cuboid::new(0.3, 0.3, 9.0)),
		tracer_glow: materials.add(glow(Color::srgb(1.0, 0.9, 0.5), LinearRgba::rgb(12.0, 9.0, 3.0))),
		// A missile is a white tube with a red nose and two crossed pairs of fins.
		missile: [
			meshes.add(Cylinder::new(0.28, 3.0)),
			meshes.add(Cone::new(0.28, 0.9)),
			meshes.add(Cuboid::new(1.5, 0.06, 0.7)),
		],
		missile_paint: materials.add(Color::srgb(0.93, 0.93, 0.95)),
		missile_trim: materials.add(Color::srgb(0.8, 0.15, 0.12)),
		ring: meshes.add(Torus::new(7.6, 10.4)),
		// An arrow for speed, a shell for the weapons, and one bar of the cross for a repair.
		signs: [
			meshes.add(Cone::new(3.5, 9.0)),
			meshes.add(Capsule3d::new(1.6, 7.0)),
			meshes.add(Cuboid::new(11.0, 3.4, 3.4)),
		],
		power_glow: POWER_COLOURS.map(|colour| materials.add(glow(colour, colour.to_linear() * 8.0))),
		pillar: meshes.add(Cylinder::new(PILLAR_RADIUS, PILLAR_HEIGHT)),
		pillar_glow: POWER_COLOURS.map(|colour| {
			materials.add(StandardMaterial {
				base_color: colour.with_alpha(0.45),
				alpha_mode: AlphaMode::Add,
				unlit: true,
				cull_mode: None,
				..default()
			})
		}),
	});
}

/// Load the plane models that the build wrote beside the page, and fill in each one when its file arrived.
fn load_plane_models(
	mut models: ResMut<PlaneModels>,
	mut asked: Local<bool>,
	assets: Res<AssetServer>,
	files: Res<Assets<Gltf>>,
	meshes: Res<Assets<GltfMesh>>,
	materials: Res<Assets<GltfMaterial>>,
) {
	if !*asked {
		*asked = true;
		page::fetch_text("assets/planes/planes.json", |text| {
			MODEL_LIST.with_borrow_mut(|list| *list = text)
		});
	}
	if let Some(text) = MODEL_LIST.with_borrow_mut(Option::take) {
		let entries: Vec<ModelEntry> = serde_json::from_str(&text).unwrap_or_default();
		models.0 = entries
			.into_iter()
			.map(|entry| PlaneModel {
				kills: entry.kills,
				propeller: entry.propeller,
				file: assets.load(format!("planes/{}", entry.file)),
				parts: None,
			})
			.collect();
	}
	for model in models.0.iter_mut().filter(|model| model.parts.is_none()) {
		let Some(file) = files.get(&model.file) else { continue };
		// The build names each mesh after its part.
		let part_meshes = Part::ALL.map(|part| {
			file.named_meshes
				.get(format!("{part:?}").as_str())
				.and_then(|mesh| meshes.get(mesh))
		});
		let Some(primitives) = part_meshes
			.iter()
			.map(|mesh| mesh.as_ref()?.primitives.first())
			.collect::<Option<Vec<_>>>()
		else {
			continue;
		};
		let texture = primitives[0]
			.material
			.as_ref()
			.and_then(|material| materials.get(material))
			.and_then(|material| material.base_color_texture.clone());
		let handles: Vec<Handle<Mesh>> = primitives.iter().map(|primitive| primitive.mesh.clone()).collect();
		if let Ok(parts) = handles.try_into() {
			model.parts = Some((parts, texture));
		}
	}
}

/// Return the ground of the rules, with the picture of the terrain laid over it.
///
/// The mesh has one corner for each point of the height grid, so a plane touches the ground where the picture shows it.
fn ground() -> Mesh {
	let grid = arena::ground();
	let side = grid.side();
	let cells = (side - 1) as f32;
	let spacing = tuning::TERRAIN_SIDE / cells;
	let place = |index: usize| (index as f32 / cells - 0.5) * tuning::TERRAIN_SIDE;
	let (mut positions, mut normals, mut picture) = (Vec::new(), Vec::new(), Vec::new());
	for row in 0..side {
		for column in 0..side {
			positions.push([place(column), grid.point(column, row), place(row)]);
			// The slope between the two neighbours of the point on each axis. A point of the edge is its own neighbour.
			let (west, east) = (column.saturating_sub(1), (column + 1).min(side - 1));
			let (north, south) = (row.saturating_sub(1), (row + 1).min(side - 1));
			let across = (grid.point(east, row) - grid.point(west, row)) / (east - west) as f32;
			let down = (grid.point(column, south) - grid.point(column, north)) / (south - north) as f32;
			normals.push(Vec3::new(-across, spacing, -down).normalize().to_array());
			// The top row of the picture is the north edge of the terrain.
			picture.push([column as f32 / cells, row as f32 / cells]);
		}
	}
	let mut indices = Vec::new();
	for row in 0..side - 1 {
		for column in 0..side - 1 {
			let corner = (row * side + column) as u32;
			let (east, south) = (corner + 1, corner + side as u32);
			indices.extend([corner, south, south + 1, corner, south + 1, east]);
		}
	}
	Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
		.with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
		.with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
		.with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, picture)
		.with_inserted_indices(Indices::U32(indices))
}

fn make_arena(
	mut commands: Commands,
	assets: Res<AssetServer>,
	mut meshes: ResMut<Assets<Mesh>>,
	mut materials: ResMut<Assets<StandardMaterial>>,
) {
	commands.spawn((
		Mesh3d(meshes.add(ground())),
		MeshMaterial3d(materials.add(StandardMaterial {
			base_color_texture: Some(assets.load("terrain/map.png")),
			perceptual_roughness: 1.0,
			reflectance: 0.1,
			..default()
		})),
	));

	// Red and white posts mark the edge of the arena, where a plane turns back by itself.
	let post = meshes.add(Cuboid::new(8.0, 160.0, 8.0));
	let paints = [Color::srgb(0.85, 0.15, 0.12), Color::srgb(0.95, 0.95, 0.95)].map(|colour| materials.add(colour));
	for index in 0..96 {
		let angle = index as f32 / 96.0 * std::f32::consts::TAU;
		let (x, z) = (angle.cos() * tuning::ARENA_RADIUS, angle.sin() * tuning::ARENA_RADIUS);
		commands.spawn((
			Mesh3d(post.clone()),
			MeshMaterial3d(paints[index % 2].clone()),
			Transform::from_xyz(x, arena::ground_height(x, z) + 80.0, z),
		));
	}
}

fn make_camera(mut commands: Commands) {
	commands.spawn((
		Camera3d::default(),
		Projection::Perspective(PerspectiveProjection {
			fov: FIELD_OF_VIEW,
			near: 0.5,
			far: 9000.0,
			..default()
		}),
		Transform::from_xyz(0.0, 400.0, 900.0).looking_at(Vec3::ZERO, Vec3::Y),
	));
}

/// Return the planes of the running match, each a part of a step ahead, as the frame shows them.
///
/// The rules move in steps, and a frame can fall between two steps. Each plane then shows a
/// little further along its course, so that motion stays smooth at every frame rate.
fn shown_planes(game: &Game) -> HashMap<PlaneId, PlaneState> {
	let ahead = game.step_fraction() * tuning::STEP;
	let planes = game
		.session
		.as_ref()
		.and_then(Session::game)
		.map(sim::Match::planes)
		.unwrap_or_default();
	planes
		.into_iter()
		.map(|(id, state)| {
			(
				id,
				PlaneState {
					position: state.position + state.velocity() * ahead,
					..state
				},
			)
		})
		.collect()
}

/// Return the colour of a plane: yellow for the player, a hue for each other player, and dull red for a bot.
fn paint(id: PlaneId, me: PlaneId) -> Color {
	match id {
		_ if id == me => Color::srgb(0.95, 0.75, 0.15),
		PlaneId::Player(number) => Color::hsl((number as f32 * 67.0 + 190.0) % 360.0, 0.7, 0.55),
		PlaneId::Bot(_) => Color::srgb(0.60, 0.30, 0.27),
	}
}

/// What a plane is drawn from: the boxes of the rules, the models of the build, and the blur of a propeller.
#[derive(SystemParam)]
struct Looks<'w> {
	models: Res<'w, Models>,
	plane_models: Res<'w, PlaneModels>,
	blur: Res<'w, Blur>,
}

fn show_planes(
	mut commands: Commands,
	game: Res<Game>,
	looks: Looks,
	mut materials: ResMut<Assets<StandardMaterial>>,
	mut planes: Query<(Entity, &PlaneView, &mut Transform, &mut Visibility), Without<PartView>>,
	mut parts: Query<(&PartView, &mut Visibility), Without<PlaneView>>,
) {
	let states = shown_planes(&game);
	let me = game.session.as_ref().and_then(Session::game).map(sim::Match::me);
	let mut drawn = HashSet::new();
	for (entity, view, mut transform, mut visibility) in &mut planes {
		// A plane that left, and a plane whose pilot earned another model, get a new picture.
		let state = states
			.get(&view.0)
			.filter(|state| looks.plane_models.for_kills(state.kills) == view.1);
		let Some(state) = state else {
			commands.entity(entity).despawn();
			continue;
		};
		drawn.insert(view.0);
		*transform = Transform::from_translation(state.position).with_rotation(state.orientation);
		*visibility = if state.alive {
			Visibility::Inherited
		} else {
			Visibility::Hidden
		};
	}
	for (part, mut visibility) in &mut parts {
		let broken = states.get(&part.0).is_some_and(|state| state.is_broken(part.1));
		*visibility = if broken {
			Visibility::Hidden
		} else {
			Visibility::Inherited
		};
	}
	for (&id, state) in states.iter().filter(|(id, _)| !drawn.contains(*id)) {
		let colour = paint(id, me.unwrap_or(id));
		let model = looks.plane_models.for_kills(state.kills);
		let loaded = model.and_then(|index| looks.plane_models.0[index].parts.as_ref());
		let propeller = model.and_then(|index| looks.plane_models.0[index].propeller);
		let body = materials.add(match loaded {
			// The texture of the model shows through a light tint in the colour of the pilot.
			Some((_, texture)) => StandardMaterial {
				base_color: Color::WHITE.mix(&colour, 0.45),
				base_color_texture: texture.clone(),
				perceptual_roughness: 0.9,
				..default()
			},
			None => colour.into(),
		});
		let root = (
			PlaneView(id, model),
			Transform::from_translation(state.position).with_rotation(state.orientation),
			Visibility::Hidden,
		);
		commands.spawn(root).with_children(|plane| {
			for part in Part::ALL {
				let piece = match loaded {
					// A mesh of a model is already in the frame of the plane.
					Some((meshes, _)) => (
						Mesh3d(meshes[part as usize].clone()),
						MeshMaterial3d(body.clone()),
						Transform::IDENTITY,
					),
					None => {
						let material = if part == Part::Engine {
							looks.models.engine.clone()
						} else {
							body.clone()
						};
						(
							Mesh3d(looks.models.parts[part as usize].clone()),
							MeshMaterial3d(material),
							Transform::from_translation(part.centre()),
						)
					}
				};
				let mut piece = plane.spawn((PartView(id, part), piece));
				// The propeller stops with its engine: the blur is a child of that part, so it hides with it.
				if let (Part::Engine, Some(propeller)) = (part, propeller) {
					piece.with_child(looks.blur.disc(propeller.centre, propeller.radius));
				}
			}
		});
	}
}

fn show_tracers(
	mut commands: Commands,
	game: Res<Game>,
	models: Res<Models>,
	mut tracers: Query<(&mut Transform, &mut Visibility), With<Tracer>>,
) {
	let bullets = game
		.session
		.as_ref()
		.and_then(Session::game)
		.map(sim::Match::bullets)
		.unwrap_or_default();
	let ahead = game.step_fraction() * tuning::STEP;
	let mut pool = tracers.iter_mut();
	for bullet in bullets {
		let place =
			Transform::from_translation(bullet.position + bullet.velocity * ahead).looking_to(bullet.velocity, Vec3::Y);
		match pool.next() {
			Some((mut transform, mut visibility)) => (*transform, *visibility) = (place, Visibility::Inherited),
			None => {
				commands.spawn((
					Tracer,
					Mesh3d(models.tracer.clone()),
					MeshMaterial3d(models.tracer_glow.clone()),
					place,
				));
			}
		}
	}
	for (_, mut visibility) in pool {
		*visibility = Visibility::Hidden;
	}
}

fn show_missiles(
	mut commands: Commands,
	game: Res<Game>,
	models: Res<Models>,
	mut missiles: Query<(&mut Transform, &mut Visibility), With<MissileView>>,
) {
	let flying = game
		.session
		.as_ref()
		.and_then(Session::game)
		.map(sim::Match::missiles)
		.unwrap_or_default();
	let mut pool = missiles.iter_mut();
	for missile in flying {
		let place = Transform::from_translation(missile.position).looking_to(missile.direction, Vec3::Y);
		match pool.next() {
			Some((mut transform, mut visibility)) => (*transform, *visibility) = (place, Visibility::Inherited),
			None => {
				// The shapes of the engine stand upright, and a missile flies toward its own -Z.
				let along = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
				let [body, nose, fins] = models.missile.clone();
				commands
					.spawn((MissileView, place, Visibility::Inherited))
					.with_children(|missile| {
						missile.spawn((
							Mesh3d(body),
							MeshMaterial3d(models.missile_paint.clone()),
							Transform::from_rotation(along),
						));
						missile.spawn((
							Mesh3d(nose),
							MeshMaterial3d(models.missile_trim.clone()),
							Transform::from_xyz(0.0, 0.0, -1.95).with_rotation(along),
						));
						for turn in [0.0, std::f32::consts::FRAC_PI_2] {
							missile.spawn((
								Mesh3d(fins.clone()),
								MeshMaterial3d(models.missile_trim.clone()),
								Transform::from_xyz(0.0, 0.0, 1.2).with_rotation(Quat::from_rotation_z(turn)),
							));
						}
					});
			}
		}
	}
	for (_, mut visibility) in pool {
		*visibility = Visibility::Hidden;
	}
}

/// Show each power-up that is there as a ring that turns, with the sign of its kind in it.
fn show_power_ups(
	mut commands: Commands,
	game: Res<Game>,
	models: Res<Models>,
	time: Res<Time>,
	mut icons: Query<(&PowerUpView, &mut Transform, &mut Visibility)>,
) {
	let power_ups = game
		.session
		.as_ref()
		.and_then(Session::game)
		.map(sim::Match::power_ups)
		.unwrap_or_default();
	let turn = Quat::from_rotation_y(time.elapsed_secs() * 1.2);
	// The icon swells and shrinks, because the eye finds a thing that moves.
	let swell = 1.0 + POWER_UP_SWELL * (time.elapsed_secs() * 4.0).sin();
	let mut drawn = 0;
	for (icon, mut transform, mut visibility) in &mut icons {
		drawn += 1;
		match power_ups.get(icon.0) {
			Some(power_up) if power_up.present => {
				(*transform, *visibility) = (
					Transform::from_translation(power_up.position)
						.with_rotation(turn)
						.with_scale(Vec3::new(swell, 1.0, swell)),
					Visibility::Inherited,
				);
			}
			_ => *visibility = Visibility::Hidden,
		}
	}
	for (place, power_up) in power_ups.iter().enumerate().skip(drawn) {
		let kind = power_up.kind as usize;
		let (ring, sign, glow) = (
			models.ring.clone(),
			models.signs[kind].clone(),
			models.power_glow[kind].clone(),
		);
		let pillar = (
			Mesh3d(models.pillar.clone()),
			MeshMaterial3d(models.pillar_glow[kind].clone()),
		);
		// The ring stands upright, so a pilot sees it from the side as a gate to fly through.
		let upright = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
		commands
			.spawn((
				PowerUpView(place),
				Transform::from_translation(power_up.position),
				Visibility::Hidden,
			))
			.with_children(|icon| {
				icon.spawn((
					Mesh3d(ring),
					MeshMaterial3d(glow.clone()),
					Transform::from_rotation(upright),
				));
				if power_up.kind == sim::PowerUpKind::Repair {
					// The second bar of the cross.
					icon.spawn((
						Mesh3d(sign.clone()),
						MeshMaterial3d(glow.clone()),
						Transform::from_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
					));
				}
				icon.spawn((Mesh3d(sign), MeshMaterial3d(glow)));
				icon.spawn(pillar);
			});
	}
}

/// Hold the camera behind the plane of the player, and tell the input what the camera sees.
///
/// The camera looks along the nose with a level horizon, so the centre of the picture is the
/// direction of flight, and the place under the cursor is the direction of the aim.
/// How the frames went while no match showed, to find a device that is too slow for the picture behind the menus.
struct Pace {
	/// The frames in a row that took longer than [`SLOW_FRAME`].
	slow_frames: u32,
	/// Whether the camera rests while no match shows. A device that was slow once stays slow.
	rests: bool,
}

impl Default for Pace {
	fn default() -> Self {
		Self {
			slow_frames: 0,
			// A phone gives its menus a still sky from the start. Waiting for forty slow frames
			// can leave a stalled device without an answer for minutes.
			rests: page::is_phone() || page::query("rest").as_deref() == Some("1"),
		}
	}
}

fn follow(
	mut game: ResMut<Game>,
	time: Res<Time>,
	camera: Single<(&mut Transform, &Projection, &mut Camera), With<Camera3d>>,
	mut pace: Local<Pace>,
) {
	let (mut transform, projection, mut camera) = camera.into_inner();
	// A menu must answer at once on every device. A device that draws the sky too slowly draws
	// nothing while no match shows, and the page shows a still sky in its place.
	let idle = game.session.as_ref().and_then(Session::game).is_none();
	if idle {
		pace.slow_frames = if time.delta_secs() > SLOW_FRAME {
			pace.slow_frames + 1
		} else {
			0
		};
		pace.rests |= pace.slow_frames >= SLOW_FRAMES;
	}
	let rests = idle && pace.rests;
	if camera.is_active == rests {
		camera.is_active = !rests;
	}
	page::set_class("game", "resting", rests);
	if let Projection::Perspective(perspective) = projection {
		(game.lens.half_height, game.lens.aspect) = ((perspective.fov / 2.0).tan(), perspective.aspect_ratio);
	}
	game.lens.frame = transform.rotation;
	let me = game.session.as_ref().and_then(Session::game).map(sim::Match::me);
	let Some(plane) = me.and_then(|me| shown_planes(&game).remove(&me)) else {
		// No match runs: the camera circles the arena.
		let angle = time.elapsed_secs() * 0.04;
		*transform = Transform::from_xyz(angle.cos() * 1100.0, 420.0, angle.sin() * 1100.0)
			.looking_at(Vec3::new(0.0, 160.0, 0.0), Vec3::Y);
		return;
	};
	if !plane.alive {
		return;
	}
	// A nose that points straight up or down has no level horizon. Near that, the camera keeps
	// the side that it had, so the picture does not spin with the plane.
	let nose = plane.forward();
	let level = nose.cross(Vec3::Y);
	let kept = (transform.rotation * Vec3::X)
		.reject_from_normalized(nose)
		.normalize_or(Vec3::X);
	let trust = ((level.length() - STEEP_START) / (STEEP_END - STEEP_START)).clamp(0.0, 1.0);
	let right = kept.lerp(level.normalize_or(kept), trust).normalize_or(kept);
	let frame = Quat::from_mat3(&Mat3::from_cols(right, right.cross(nose), -nose)).normalize();
	let place = plane.position + frame * CHASE_OFFSET;
	// A respawn moves the camera at once. Flight moves it smoothly.
	let pace = if transform.translation.distance(place) > 200.0 {
		1.0
	} else {
		1.0 - (-CHASE_PACE * time.delta_secs()).exp()
	};
	transform.translation = transform.translation.lerp(place, pace);
	transform.rotation = frame;
	game.lens.frame = frame;
}

use bevy::prelude::*;
use sim::flight::{Part, Parts, PARTS};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
pub struct RenderPlane {
    pub pos: sim::Vec3,
    pub rot: sim::Quat,
    pub parts: Parts,
    pub alive: bool,
    pub protected: bool,
    pub speed: f32,
}

#[derive(Resource, Default)]
pub struct RenderState {
    pub own: Option<u32>,
    pub lock_pct: u8,
    pub planes: HashMap<u32, RenderPlane>,
    pub missiles: HashMap<u32, (sim::Vec3, sim::Quat)>,
    pub tracers: Vec<(sim::Vec3, sim::Vec3)>,
    pub bursts: Vec<sim::Vec3>,
    pub clear: bool,
}

#[derive(Resource)]
pub(crate) struct Shapes {
    cube: Handle<Mesh>,
    sphere: Handle<Mesh>,
    plane_mats: Vec<Handle<StandardMaterial>>,
    broken: Handle<StandardMaterial>,
    missile: Handle<StandardMaterial>,
    tracer: Handle<StandardMaterial>,
    burst: Handle<StandardMaterial>,
}

#[derive(Default, Resource)]
pub(crate) struct Visuals {
    planes: HashMap<u32, PlaneVisual>,
    missiles: HashMap<u32, Entity>,
}

struct PlaneVisual {
    root: Entity,
    parts: HashMap<Part, Entity>,
}

#[derive(Component)]
pub(crate) struct Transient {
    ttl: f32,
}

pub fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 350.0, 600.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight { illuminance: 12_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.8, -0.5, 0.0)),
    ));
    commands.spawn(AmbientLight { color: Color::srgb(0.35, 0.45, 0.65), brightness: 500.0, ..default() });
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(6000.0, 1.0, 6000.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.08, 0.18, 0.12),
            perceptual_roughness: 0.9,
            ..default()
        })),
        Transform::from_xyz(0.0, -20.0, 0.0),
    ));

    let plane_mats = [
        Color::srgb(0.25, 0.55, 1.0),
        Color::srgb(1.0, 0.35, 0.25),
        Color::srgb(0.25, 0.9, 0.45),
        Color::srgb(1.0, 0.75, 0.2),
        Color::srgb(0.75, 0.4, 1.0),
        Color::srgb(0.2, 0.85, 0.9),
    ]
    .into_iter()
    .map(|base_color| {
        materials.add(StandardMaterial { base_color, metallic: 0.1, perceptual_roughness: 0.45, ..default() })
    })
    .collect();
    let broken = materials.add(StandardMaterial { base_color: Color::srgb(0.12, 0.12, 0.12), ..default() });
    let missile = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.35, 0.1),
        emissive: LinearRgba::new(4.0, 0.5, 0.05, 1.0),
        ..default()
    });
    let tracer = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.9, 0.2),
        emissive: LinearRgba::new(5.0, 4.0, 0.2, 1.0),
        unlit: true,
        ..default()
    });
    let burst = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.25, 0.05, 0.45),
        emissive: LinearRgba::new(5.0, 0.6, 0.05, 1.0),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    commands.insert_resource(Shapes {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        sphere: meshes.add(Sphere::new(1.0)),
        plane_mats,
        broken,
        missile,
        tracer,
        burst,
    });
    commands.init_resource::<Visuals>();
}

fn v3(v: sim::Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn quat(q: sim::Quat) -> Quat {
    Quat::from_xyzw(q.x, q.y, q.z, q.w)
}

pub fn sync_render(
    mut commands: Commands,
    mut render: ResMut<RenderState>,
    shapes: Res<Shapes>,
    mut visuals: ResMut<Visuals>,
    mut transforms: Query<&mut Transform, Without<Camera3d>>,
    mut visibility: Query<&mut Visibility>,
    mut camera: Query<&mut Transform, With<Camera3d>>,
) {
    if render.clear {
        for visual in visuals.planes.values() {
            commands.entity(visual.root).despawn();
        }
        for entity in visuals.missiles.values() {
            commands.entity(*entity).despawn();
        }
        visuals.planes.clear();
        visuals.missiles.clear();
        render.clear = false;
    }

    let present: Vec<u32> = render.planes.keys().copied().collect();
    let absent: Vec<u32> = visuals
        .planes
        .keys()
        .copied()
        .filter(|id| !present.contains(id))
        .collect();
    for id in absent {
        if let Some(visual) = visuals.planes.remove(&id) {
            commands.entity(visual.root).despawn();
        }
    }

    for (id, plane) in &render.planes {
        let visual = visuals.planes.entry(*id).or_insert_with(|| {
            let root = commands
                .spawn((Transform::default(), GlobalTransform::default(), Visibility::Visible))
                .id();
            let material = shapes.plane_mats[(*id as usize) % shapes.plane_mats.len()].clone();
            let mut parts = HashMap::new();
            for part in PARTS {
                let (scale, material) = match part {
                    Part::Fuselage => (Vec3::new(2.2, 1.4, 10.0), material.clone()),
                    Part::LeftWing | Part::RightWing => (Vec3::new(6.5, 0.25, 3.0), material.clone()),
                    Part::Tail => (Vec3::new(2.0, 1.8, 0.25), shapes.broken.clone()),
                    Part::Engine => (Vec3::new(1.3, 1.3, 2.0), shapes.broken.clone()),
                };
                let entity = commands
                    .spawn((
                        Mesh3d(shapes.cube.clone()),
                        MeshMaterial3d(material),
                        Transform::from_translation(v3(part.offset())).with_scale(scale),
                    ))
                    .id();
                commands.entity(root).add_child(entity);
                parts.insert(part, entity);
            }
            PlaneVisual { root, parts }
        });
        if let Ok(mut transform) = transforms.get_mut(visual.root) {
            *transform = Transform::from_translation(v3(plane.pos)).with_rotation(quat(plane.rot));
        }
        if let Ok(mut visible) = visibility.get_mut(visual.root) {
            *visible = if plane.alive { Visibility::Visible } else { Visibility::Hidden };
        }
        for part in PARTS {
            if let Some(entity) = visual.parts.get(&part)
                && let Ok(mut visible) = visibility.get_mut(*entity)
            {
                *visible = if plane.parts.health(part) > 0 && plane.alive {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
            }
        }
    }

    let present: Vec<u32> = render.missiles.keys().copied().collect();
    let absent: Vec<u32> = visuals
        .missiles
        .keys()
        .copied()
        .filter(|id| !present.contains(id))
        .collect();
    for id in absent {
        if let Some(entity) = visuals.missiles.remove(&id) {
            commands.entity(entity).despawn();
        }
    }
    for (id, (pos, rot)) in &render.missiles {
        let entity = *visuals.missiles.entry(*id).or_insert_with(|| {
            commands
                .spawn((
                    Mesh3d(shapes.cube.clone()),
                    MeshMaterial3d(shapes.missile.clone()),
                    Transform::from_scale(Vec3::new(0.7, 0.7, 3.0)),
                ))
                .id()
        });
        if let Ok(mut transform) = transforms.get_mut(entity) {
            *transform = Transform::from_translation(v3(*pos))
                .with_rotation(quat(*rot))
                .with_scale(Vec3::new(0.7, 0.7, 3.0));
        }
    }

    for (from, to) in render.tracers.drain(..) {
        let from = v3(from);
        let to = v3(to);
        let dir = to - from;
        let length = dir.length();
        if length < 0.1 {
            continue;
        }
        commands.spawn((
            Mesh3d(shapes.cube.clone()),
            MeshMaterial3d(shapes.tracer.clone()),
            Transform::from_translation((from + to) * 0.5)
                .looking_to(dir / length, Vec3::Y)
                .with_scale(Vec3::new(0.08, 0.08, length)),
            Transient { ttl: 0.08 },
        ));
    }
    for pos in render.bursts.drain(..) {
        commands.spawn((
            Mesh3d(shapes.sphere.clone()),
            MeshMaterial3d(shapes.burst.clone()),
            Transform::from_translation(v3(pos)).with_scale(Vec3::splat(9.0)),
            Transient { ttl: 0.35 },
        ));
    }

    if let Some(own) = render.own.and_then(|id| render.planes.get(&id)) {
        let rot = quat(own.rot);
        let pos = v3(own.pos);
        let forward = rot * Vec3::NEG_Z;
        let target = pos + forward * 35.0;
        let camera_pos = pos - forward * 30.0 + Vec3::Y * 10.0;
        for mut transform in &mut camera {
            transform.translation = transform.translation.lerp(camera_pos, 0.2);
            transform.look_at(target, Vec3::Y);
        }
    }
}

pub fn update_transients(
    time: Res<Time>,
    mut commands: Commands,
    mut transients: Query<(Entity, &mut Transient)>,
) {
    for (entity, mut transient) in &mut transients {
        transient.ttl -= time.delta_secs();
        if transient.ttl <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

use crate::input;
use crate::model::{Bindings, Mode, Results, Screen, Stats, TouchState};
use crate::render::RenderState;
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use sim::bots::{self, Skill};
use sim::flight::Controls;
use sim::missile::Lock;
use sim::world::{Event, World};
use std::collections::HashMap;

#[derive(Resource)]
pub struct SoloGame {
    pub world: World,
    pub skill: Skill,
    pub locks: HashMap<u32, Lock>,
    pub scores: HashMap<u32, (u32, u32)>,
    pub pilot_bot: bool,
    pub auto_end: Option<f32>,
    pub ended: bool,
    accumulator: f32,
}

impl SoloGame {
    pub fn new(bots: u8, skill: Skill, pilot_bot: bool, auto_end: Option<f32>) -> Self {
        let mut world = World::new();
        world.spawn(1);
        let mut scores = HashMap::new();
        scores.insert(1, (0, 0));
        for bot in 0..bots {
            let id = 2 + bot as u32;
            world.spawn(id);
            scores.insert(id, (0, 0));
        }
        Self {
            world,
            skill,
            locks: HashMap::new(),
            scores,
            pilot_bot,
            auto_end,
            ended: false,
            accumulator: 0.0,
        }
    }
}

pub fn step_solo(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    wheel: Res<AccumulatedMouseScroll>,
    windows: Query<&Window, With<PrimaryWindow>>,
    bindings: Res<Bindings>,
    mut touch: ResMut<TouchState>,
    screen: Res<Screen>,
    mode: Res<Mode>,
    solo: Option<ResMut<SoloGame>>,
    mut render: ResMut<RenderState>,
    mut stats: ResMut<Stats>,
    mut results: ResMut<Results>,
) {
    if *mode != Mode::Solo || (*screen != Screen::Match && *screen != Screen::Paused) {
        return;
    }
    let Some(mut solo) = solo else {
        return;
    };
    if *screen == Screen::Paused {
        sync_render(&solo, &mut render);
        return;
    }

    let dt_frame = time.delta_secs().min(0.1);
    solo.accumulator += dt_frame;
    let step = 1.0 / 120.0;
    while solo.accumulator >= step {
        solo.accumulator -= step;
        let planes: Vec<_> = solo.world.planes.values().cloned().collect();
        let mut controls: HashMap<u32, Controls> = HashMap::new();
        for plane in &planes {
            let input = if plane.id == 1 && !solo.pilot_bot {
                input::gather(
                    &keys,
                    &mouse,
                    &touches,
                    &wheel,
                    windows.single().ok(),
                    &bindings,
                    &mut touch,
                    solo.world.time,
                    plane,
                    step,
                )
            } else {
                let target = bots::pick_target(plane, planes.iter(), solo.world.time).cloned();
                let action = bots::fly(plane, solo.skill, target.as_ref(), solo.world.time);
                input::PlayerInput {
                    controls: action.controls,
                    fire: action.fire,
                    missile: action.wants_launch,
                }
            };
            controls.insert(plane.id, input.controls);

            let candidates = planes.iter().filter(|other| other.id != plane.id);
            let now = solo.world.time;
            let locked = {
                let lock = solo.locks.entry(plane.id).or_default();
                lock.update(step, plane, candidates, now);
                lock.locked()
            };
            if input.fire {
                let events = solo.world.fire(plane.id);
                if events.is_empty() && plane.alive {
                    render.tracers.push((
                        plane.pos + plane.forward() * 6.0,
                        plane.pos + plane.forward() * sim::tuning::GUN_RANGE,
                    ));
                }
                handle_events(&events, &mut solo, &mut render, &mut stats);
            }
            if input.missile && let Some(target) = locked {
                let allowed = if plane.id == 1 {
                    true
                } else {
                    solo.skill.missile_range().is_some_and(|range| {
                        solo.world
                            .planes
                            .get(&target)
                            .is_some_and(|p| (p.pos - plane.pos).length() < range)
                    })
                };
                if allowed && let Some(event) = solo.world.launch(plane.id, target) {
                    if let Some(lock) = solo.locks.get_mut(&plane.id) {
                        lock.reset();
                    }
                    stats.missiles += 1;
                    handle_events(&[event], &mut solo, &mut render, &mut stats);
                }
            }
        }
        let events = solo.world.step(step, &controls);
        handle_events(&events, &mut solo, &mut render, &mut stats);
        let events = solo.world.step_missiles(step);
        handle_events(&events, &mut solo, &mut render, &mut stats);
    }
    sync_render(&solo, &mut render);

    if !solo.ended && solo.auto_end.is_some_and(|seconds| solo.world.time >= seconds) {
        solo.ended = true;
        crate::hooks::log(&stats.report());
        results.0 = score_lines(&solo.scores);
    }
}

fn handle_events(
    events: &[Event],
    solo: &mut SoloGame,
    render: &mut RenderState,
    stats: &mut Stats,
) {
    for event in events {
        match *event {
            Event::Hit { shooter, target, .. } => {
                stats.hits_sent += 1;
                stats.hits_applied += 1;
                let from = solo.world.planes.get(&shooter).map(|p| p.pos);
                let to = solo.world.planes.get(&target).map(|p| p.pos);
                if let (Some(from), Some(to)) = (from, to) {
                    render.tracers.push((from, to));
                }
            }
            Event::Kill { shooter, target } => {
                stats.kills += 1;
                if target == 1 {
                    stats.deaths += 1;
                }
                solo.scores.entry(shooter).or_default().0 += 1;
                solo.scores.entry(target).or_default().1 += 1;
            }
            Event::Burst { pos, .. } => render.bursts.push(pos),
            Event::Launch { .. } | Event::Broken { .. } | Event::Respawn { .. } => {}
        }
    }
}

fn sync_render(solo: &SoloGame, render: &mut RenderState) {
    render.own = Some(1);
    render.lock_pct = solo
        .locks
        .get(&1)
        .filter(|l| l.target.is_some())
        .map(|l| ((l.held / sim::tuning::LOCK_TIME) * 100.0).clamp(0.0, 100.0) as u8)
        .unwrap_or(0);
    render.planes = solo
        .world
        .planes
        .values()
        .map(|p| {
            (
                p.id,
                crate::render::RenderPlane {
                    pos: p.pos,
                    rot: p.rot(),
                    parts: p.parts,
                    alive: p.alive,
                    protected: p.protected(solo.world.time),
                    speed: p.speed(),
                },
            )
        })
        .collect();
    render.missiles = solo
        .world
        .missiles
        .iter()
        .map(|m| {
            let rot = sim::Quat::from_rotation_arc(sim::Vec3::NEG_Z, m.vel.normalize_or_zero());
            (m.id, (m.pos, rot))
        })
        .collect();
}

pub fn score_lines(scores: &HashMap<u32, (u32, u32)>) -> Vec<String> {
    let mut scores: Vec<_> = scores.iter().collect();
    scores.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    scores
        .into_iter()
        .map(|(id, (kills, deaths))| {
            let name = if *id == 1 { "You".into() } else { format!("Bot {id}") };
            format!("{name}: {kills} kills, {deaths} deaths")
        })
        .collect()
}

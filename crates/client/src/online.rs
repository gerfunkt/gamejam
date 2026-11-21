use crate::input;
use crate::model::{Bindings, Mode, Results, Screen, SharedView, Stats, TouchState};
use crate::net::{self, Net, NetEvent};
use crate::render::{RenderPlane, RenderState};
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use sim::bots::{self, Skill};
use sim::combat::{self, Outcome};
use sim::flight::{Controls, Plane};
use sim::lobby::{Lobby, Member};
use sim::missile::{Lock, Missile};
use sim::timeline::{Sample, Timeline};
use sim::wire::{Message, State};
use sim::{Quat, Vec3};
use std::collections::HashMap;

const BOT_BASE: u32 = 1_000_000;

pub struct RemotePlane {
    timeline: Timeline,
    sample: Option<Sample>,
    dead: bool,
}

pub(crate) struct NetMissile {
    missile: Missile,
    local: bool,
}

#[derive(Resource)]
pub struct OnlineGame {
    pub you: u32,
    pub time: f32,
    pub me: Plane,
    pub bots: HashMap<u32, Plane>,
    pub remotes: HashMap<u32, RemotePlane>,
    pub locks: HashMap<u32, Lock>,
    pub missiles: HashMap<u32, NetMissile>,
    pub scores: HashMap<u32, (u32, u32)>,
    pub skill: Skill,
    pub pilot_bot: bool,
    pub auto_end: Option<f32>,
    pub ended: bool,
    accumulator: f32,
    send_accumulator: f32,
    next_missile: u32,
}

impl OnlineGame {
    pub fn new(you: u32, lobby: &Lobby, pilot_bot: bool, auto_end: Option<f32>) -> Self {
        let mut game = Self {
            you,
            time: 0.0,
            me: spawn_plane(you, 0.0),
            bots: HashMap::new(),
            remotes: HashMap::new(),
            locks: HashMap::new(),
            missiles: HashMap::new(),
            scores: HashMap::new(),
            skill: lobby.skill,
            pilot_bot,
            auto_end,
            ended: false,
            accumulator: 0.0,
            send_accumulator: 0.0,
            next_missile: 0,
        };
        for member in &lobby.members {
            game.scores.insert(member.num, (0, 0));
            if member.num != you {
                game.remotes.insert(member.num, RemotePlane {
                    timeline: Timeline::new(),
                    sample: None,
                    dead: false,
                });
            }
        }
        game.ensure_bots(lobby);
        game
    }

    fn ensure_bots(&mut self, lobby: &Lobby) {
        let owns_bots = lobby.host() == Some(self.you);
        if owns_bots {
            for i in 0..lobby.bots {
                let id = BOT_BASE + i as u32;
                self.bots.entry(id).or_insert_with(|| spawn_plane(id, self.time));
                self.scores.entry(id).or_default();
            }
            let wanted = lobby.bots as u32;
            self.bots.retain(|id, _| *id < BOT_BASE + wanted);
        } else {
            self.bots.clear();
        }
    }

    fn all_planes(&self) -> Vec<Plane> {
        let mut planes = vec![self.me.clone()];
        planes.extend(self.bots.values().cloned());
        planes.extend(self.remotes.iter().filter_map(|(id, remote)| {
            remote.sample.map(|sample| remote_plane(*id, sample, remote.dead))
        }));
        planes
    }

    fn target_plane(&self, id: u32) -> Option<Plane> {
        if id == self.you {
            Some(self.me.clone())
        } else if let Some(plane) = self.bots.get(&id) {
            Some(plane.clone())
        } else {
            self.remotes.get(&id).and_then(|remote| {
                remote.sample.map(|sample| remote_plane(id, sample, remote.dead))
            })
        }
    }

    fn local_mut(&mut self, id: u32) -> Option<&mut Plane> {
        if id == self.you {
            Some(&mut self.me)
        } else {
            self.bots.get_mut(&id)
        }
    }

    fn spawn_local(&mut self, id: u32) {
        let plane = spawn_plane(id, self.time);
        if id == self.you {
            self.me = plane;
        } else if let Some(slot) = self.bots.get_mut(&id) {
            *slot = plane;
        }
    }
}

fn spawn_plane(id: u32, time: f32) -> Plane {
    let angle = id as f32 * 2.4;
    let slot = (id % 16) as f32;
    let pos = Vec3::new(angle.cos() * 400.0, 200.0 + 30.0 * slot, angle.sin() * 400.0);
    Plane::new(id, pos, Quat::from_rotation_y(angle), time)
}

fn remote_plane(id: u32, sample: Sample, dead: bool) -> Plane {
    let mut plane = Plane::new(id, sample.pos, sample.rot, 0.0);
    plane.vel = sample.vel;
    plane.parts = sample.parts;
    plane.alive = !dead;
    plane.protect_until = 0.0;
    plane.throttle = 1.0;
    plane
}

pub fn start_online(you: u32, lobby: &Lobby, pilot_bot: bool, auto_end: Option<f32>) -> OnlineGame {
    OnlineGame::new(you, lobby, pilot_bot, auto_end)
}

pub fn net_events(
    mut net: NonSendMut<Net>,
    mut shared: ResMut<SharedView>,
    mut commands: Commands,
    mut screen: ResMut<Screen>,
    mode: Res<Mode>,
    hooks: Res<crate::hooks::Hooks>,
    player: Res<crate::model::Player>,
    online: Option<ResMut<OnlineGame>>,
    mut render: ResMut<RenderState>,
    mut stats: ResMut<Stats>,
    mut results: ResMut<Results>,
) {
    let events = net.drain();
    if events.is_empty() {
        return;
    }
    let mut online = online;
    for event in events {
        match event {
            NetEvent::Open => {
                if let Some(you) = shared.0.you {
                    net::send_profile(&net, you, &player.name);
                }
            }
            NetEvent::Text(text) => {
                if let Some(status) = sim::room::parse_status(&text) {
                    let was_you = shared.0.you;
                    shared.0.you = Some(status.you);
                    shared.0.room_title = status.title;
                    shared.0.profiles.entry(status.you).or_insert_with(|| player.name.clone());
                    if was_you != Some(status.you) {
                        net::send_profile(&net, status.you, &player.name);
                    }
                    update_lobby_from_members(&mut shared.0, &status.members);
                    if let Some(game) = online.as_mut() {
                        for &m in &status.members {
                            game.scores.entry(m).or_default();
                            if m != status.you {
                                game.remotes.entry(m).or_insert_with(|| RemotePlane {
                                    timeline: Timeline::new(),
                                    sample: None,
                                    dead: false,
                                });
                            }
                        }
                        game.remotes.retain(|id, _| status.members.contains(id));
                        game.scores.retain(|id, _| status.members.contains(id) || *id >= BOT_BASE);
                        game.ensure_bots(&shared.0.lobby);
                    }
                    if shared.0.lobby.host() == Some(status.you) {
                        if hooks.auto.as_deref() == Some("online") && !shared.0.lobby.running {
                            shared.0.lobby.running = true;
                        }
                        net::send_game(&net, &Message::Lobby(shared.0.lobby.clone()));
                    }
                    if shared.0.lobby.running
                        && *mode == Mode::Online
                        && online.is_none()
                    {
                        commands.insert_resource(start_online(
                            status.you,
                            &shared.0.lobby,
                            hooks.pilot_bot,
                            hooks.seconds,
                        ));
                        *screen = Screen::Match;
                    }
                }
            }
            NetEvent::Binary(bytes) => {
                if bytes.len() < 5 {
                    continue;
                }
                let from = u32::from_le_bytes(bytes[..4].try_into().unwrap());
                let Some(message) = sim::wire::decode(&bytes[4..]) else {
                    stats.errors += 1;
                    continue;
                };
                apply_message(
                    message,
                    from,
                    &net,
                    &hooks,
                    &mut shared,
                    &mut commands,
                    &mut screen,
                    &mode,
                    &mut online,
                    &mut render,
                    &mut stats,
                    &mut results,
                );
            }
            NetEvent::Rooms(body) => {
                shared.0.rooms = sim::room::parse_list(&body);
            }
            NetEvent::Close(code, reason) => {
                if *mode == Mode::Online && *screen != Screen::Results {
                    shared.0.error = format!("Connection closed ({code}) {reason}");
                    *screen = Screen::Error;
                }
            }
            NetEvent::Error(error) => {
                stats.errors += 1;
                shared.0.error = error;
            }
        }
    }
}

fn update_lobby_from_members(shared: &mut crate::model::LobbyView, members: &[u32]) {
    let mut lobby_members = Vec::new();
    for num in members {
        let name = shared
            .profiles
            .get(num)
            .cloned()
            .unwrap_or_else(|| format!("Pilot {num}"));
        shared.profiles.insert(*num, name.clone());
        lobby_members.push(Member { num: *num, name });
    }
    shared.lobby.members = lobby_members;
}

fn apply_message(
    message: Message,
    _from: u32,
    net: &Net,
    hooks: &crate::hooks::Hooks,
    shared: &mut SharedView,
    commands: &mut Commands,
    screen: &mut Screen,
    mode: &Mode,
    online: &mut Option<ResMut<OnlineGame>>,
    render: &mut RenderState,
    stats: &mut Stats,
    results: &mut Results,
) {
    match message {
        Message::State(state) => {
            let Some(game) = online.as_mut() else {
                return;
            };
            if state.id == game.you || game.bots.contains_key(&state.id) {
                return;
            }
            let remote = game.remotes.entry(state.id).or_insert_with(|| RemotePlane {
                timeline: Timeline::new(),
                sample: None,
                dead: false,
            });
            remote.dead = false;
            remote.timeline.push(net::current_clock(), Sample {
                send: state.clock,
                pos: state.pos,
                vel: state.vel,
                rot: state.rot,
                parts: state.parts,
            });
        }
        Message::Hit { shooter, target, part, damage } => {
            let Some(game) = online.as_mut() else {
                return;
            };
            let now = game.time;
            if let Some(plane) = game.local_mut(target) {
                stats.hits_applied += 1;
                if combat::apply_damage(plane, part, damage, now) == Outcome::Killed {
                    net::send_game(net, &Message::Kill { shooter, target });
                }
            }
        }
        Message::Kill { shooter, target } => {
            stats.kills += 1;
            if let Some(game) = online.as_mut() {
                game.scores.entry(shooter).or_default().0 += 1;
                game.scores.entry(target).or_default().1 += 1;
                if target == game.you {
                    stats.deaths += 1;
                }
                if let Some(plane) = game.local_mut(target) {
                    plane.alive = false;
                    plane.respawn_in = sim::tuning::RESPAWN_S;
                } else if let Some(remote) = game.remotes.get_mut(&target) {
                    remote.dead = true;
                }
            }
        }
        Message::Launch { id, shooter, target, pos, dir } => {
            if let Some(game) = online.as_mut() {
                if shooter == game.you || game.bots.contains_key(&shooter) {
                    return;
                }
                let mut missile = Missile {
                    id,
                    shooter,
                    target,
                    pos,
                    vel: dir.normalize_or_zero() * sim::tuning::MISSILE_SPEED,
                    fuel: sim::tuning::MISSILE_FUEL,
                    grace: sim::tuning::MISSILE_GRACE,
                    blind: false,
                };
                missile.blind = false;
                game.missiles.insert(id, NetMissile { missile, local: false });
            }
        }
        Message::Burst { id, pos } => {
            render.bursts.push(pos);
            if let Some(game) = online.as_mut() {
                let shooter = game.missiles.remove(&id).map(|m| m.missile.shooter).unwrap_or(0);
                let ids: Vec<u32> = std::iter::once(game.you).chain(game.bots.keys().copied()).collect();
                let now = game.time;
                for pid in ids {
                    if let Some(plane) = game.local_mut(pid) {
                        for (_, outcome) in combat::burst_damage(plane, pos, now) {
                            if outcome == Outcome::Killed {
                                net::send_game(net, &Message::Kill { shooter, target: pid });
                            }
                        }
                    }
                }
            }
        }
        Message::EndMatch { scores } => {
            results.0 = scores
                .iter()
                .map(|(id, kills, deaths)| {
                    let name = if *id >= BOT_BASE {
                        format!("Bot {}", id - BOT_BASE + 1)
                    } else {
                        shared
                            .0
                            .profiles
                            .get(id)
                            .cloned()
                            .unwrap_or_else(|| format!("Pilot {id}"))
                    };
                    format!("{name}: {kills} kills, {deaths} deaths")
                })
                .collect();
            if let Some(game) = online.as_mut() {
                game.ended = true;
            }
            render.clear = true;
            crate::hooks::log(&stats.report());
            *screen = Screen::Results;
        }
        Message::Lobby(lobby) => {
            shared.0.lobby = lobby;
            for member in &shared.0.lobby.members {
                shared.0.profiles.insert(member.num, member.name.clone());
            }
            if shared.0.lobby.running
                && *mode == Mode::Online
                && online.is_none()
                && let Some(you) = shared.0.you
            {
                commands.insert_resource(start_online(
                    you,
                    &shared.0.lobby,
                    hooks.pilot_bot,
                    hooks.seconds,
                ));
                *screen = Screen::Match;
            }
        }
        Message::Profile { id, name } => {
            shared.0.profiles.insert(id, name);
            if let Some(member) = shared.0.lobby.members.iter_mut().find(|m| m.num == id) {
                member.name = shared.0.profiles[&id].clone();
            }
            if shared.0.lobby.host() == shared.0.you {
                net::send_game(net, &Message::Lobby(shared.0.lobby.clone()));
            }
        }
    }
}

pub fn step_online(
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
    online: Option<ResMut<OnlineGame>>,
    net: NonSendMut<Net>,
    mut shared: ResMut<SharedView>,
    mut render: ResMut<RenderState>,
    mut stats: ResMut<Stats>,
) {
    if *mode != Mode::Online || (*screen != Screen::Match && *screen != Screen::Paused) {
        return;
    }
    let Some(mut game) = online else {
        return;
    };
    game.ensure_bots(&shared.0.lobby);
    let dt_frame = time.delta_secs().min(0.1);
    game.accumulator += dt_frame;
    let step = 1.0 / 120.0;
    while game.accumulator >= step {
        game.accumulator -= step;
        game.time += step;
        for remote in game.remotes.values_mut() {
            remote.sample = remote.timeline.sample(net::current_clock());
        }
        let planes = game.all_planes();
        let mut controls: HashMap<u32, Controls> = HashMap::new();
        let owned: Vec<u32> = std::iter::once(game.you).chain(game.bots.keys().copied()).collect();
        for id in owned {
            let Some(plane) = game.target_plane(id) else {
                continue;
            };
            if !plane.alive {
                continue;
            }
            let input = if id == game.you && !game.pilot_bot {
                input::gather(
                    &keys,
                    &mouse,
                    &touches,
                    &wheel,
                    windows.single().ok(),
                    &bindings,
                    &mut touch,
                    game.time,
                    &plane,
                    step,
                )
            } else {
                let target = bots::pick_target(&plane, planes.iter(), game.time).cloned();
                let action = bots::fly(&plane, game.skill, target.as_ref(), game.time);
                input::PlayerInput {
                    controls: action.controls,
                    fire: action.fire,
                    missile: action.wants_launch,
                }
            };
            controls.insert(id, input.controls);
            let candidates = planes.iter().filter(|p| p.id != id);
            let now = game.time;
            let locked = {
                let lock = game.locks.entry(id).or_default();
                lock.update(step, &plane, candidates, now);
                lock.locked()
            };
            let cease_fire = game.auto_end.is_some_and(|s| game.time >= s - 0.4);
            if input.fire && !cease_fire {
                fire_online(&mut game, id, &net, &mut render, &mut stats);
            }
            if input.missile && !cease_fire && let Some(target) = locked {
                let allowed = if id == game.you {
                    true
                } else {
                    game.skill.missile_range().is_some_and(|range| {
                        game.target_plane(target)
                            .is_some_and(|p| (p.pos - plane.pos).length() < range)
                    })
                };
                if allowed {
                    if let Some(lock) = game.locks.get_mut(&id) {
                        lock.reset();
                    }
                    launch_online(&mut game, id, target, &net, &mut stats);
                }
            }
        }
        for (id, controls) in controls {
            if let Some(plane) = game.local_mut(id) {
                plane.step(step, &controls);
            }
        }
        step_respawns(&mut game, step);
        step_missiles(&mut game, step, &net, &mut render, &mut stats);
    }

    game.send_accumulator += dt_frame;
    if game.send_accumulator >= 1.0 / sim::tuning::STATE_HZ {
        game.send_accumulator = 0.0;
        let clock = net::current_clock();
        if game.me.alive {
            net::send_game(&net, &Message::State(State::of(&game.me, clock)));
        }
        for bot in game.bots.values().filter(|p| p.alive) {
            net::send_game(&net, &Message::State(State::of(bot, clock)));
        }
    }
    sync_render(&game, &mut render);

    if !game.ended
        && shared.0.lobby.host() == Some(game.you)
        && game.auto_end.is_some_and(|seconds| game.time >= seconds)
    {
        game.ended = true;
        let scores = score_vec(&game);
        net::send_game(&net, &Message::EndMatch { scores });
        shared.0.lobby.running = false;
        net::send_game(&net, &Message::Lobby(shared.0.lobby.clone()));
    }

    if shared.0.lobby.host() == Some(game.you) && shared.0.lobby.running != !game.ended {
        shared.0.lobby.running = !game.ended;
    }
}

fn fire_online(
    game: &mut OnlineGame,
    shooter: u32,
    net: &Net,
    render: &mut RenderState,
    stats: &mut Stats,
) {
    let Some(plane) = game.target_plane(shooter) else {
        return;
    };
    let origin = plane.pos + plane.forward() * 6.0;
    let dir = plane.forward();
    let mut best: Option<(u32, sim::flight::Part, f32)> = None;
    for target in game.all_planes() {
        if target.id == shooter || !target.alive || target.protected(game.time) {
            continue;
        }
        if let Some((part, dist)) = combat::ray_hit(origin, dir, &target)
            && best.map_or(true, |(_, _, old)| dist < old)
        {
            best = Some((target.id, part, dist));
        }
    }
    let end = if let Some((target, part, dist)) = best {
        stats.hits_sent += 1;
        net::send_game(net, &Message::Hit {
            shooter,
            target,
            part,
            damage: sim::tuning::GUN_DAMAGE,
        });
        origin + dir * dist
    } else {
        origin + dir * sim::tuning::GUN_RANGE
    };
    render.tracers.push((origin, end));
}

fn launch_online(game: &mut OnlineGame, shooter: u32, target: u32, net: &Net, stats: &mut Stats) {
    let Some(plane) = game.target_plane(shooter) else {
        return;
    };
    game.next_missile += 1;
    let id = shooter.saturating_mul(100_000).saturating_add(game.next_missile);
    let missile = Missile::launch(id, &plane, target);
    net::send_game(net, &Message::Launch {
        id,
        shooter,
        target,
        pos: missile.pos,
        dir: missile.vel.normalize_or_zero(),
    });
    game.missiles.insert(id, NetMissile { missile, local: true });
    stats.missiles += 1;
}

fn step_respawns(game: &mut OnlineGame, dt: f32) {
    let ids: Vec<u32> = std::iter::once(game.you).chain(game.bots.keys().copied()).collect();
    let mut respawn = Vec::new();
    for id in ids {
        let Some(plane) = game.local_mut(id) else {
            continue;
        };
        if !plane.alive {
            plane.respawn_in -= dt;
            if plane.respawn_in <= 0.0 {
                respawn.push(id);
            }
        }
    }
    for id in respawn {
        game.spawn_local(id);
    }
}

fn step_missiles(
    game: &mut OnlineGame,
    dt: f32,
    net: &Net,
    _render: &mut RenderState,
    _stats: &mut Stats,
) {
    let ids: Vec<u32> = game.missiles.keys().copied().collect();
    for id in ids {
        let Some((target_id, local)) = game
            .missiles
            .get(&id)
            .map(|item| (item.missile.target, item.local))
        else {
            continue;
        };
        let Some(target) = game.target_plane(target_id) else {
            continue;
        };
        let burst = {
            let Some(item) = game.missiles.get_mut(&id) else {
                continue;
            };
            item.missile.step(dt, &target)
        };
        if let Some(pos) = burst {
            if local {
                net::send_game(net, &Message::Burst { id, pos });
            }
        }
        if game.missiles.get(&id).is_some_and(|m| m.missile.expired()) {
            game.missiles.remove(&id);
        }
    }
}

fn sync_render(game: &OnlineGame, render: &mut RenderState) {
    render.own = Some(game.you);
    render.lock_pct = game
        .locks
        .get(&game.you)
        .filter(|l| l.target.is_some())
        .map(|l| ((l.held / sim::tuning::LOCK_TIME) * 100.0).clamp(0.0, 100.0) as u8)
        .unwrap_or(0);
    render.planes.clear();
    for plane in std::iter::once(&game.me).chain(game.bots.values()) {
        render.planes.insert(plane.id, RenderPlane {
            pos: plane.pos,
            rot: plane.rot(),
            parts: plane.parts,
            alive: plane.alive,
            protected: plane.protected(game.time),
            speed: plane.speed(),
        });
    }
    for (id, remote) in &game.remotes {
        if let Some(sample) = remote.sample {
            render.planes.insert(*id, RenderPlane {
                pos: sample.pos,
                rot: sample.rot,
                parts: sample.parts,
                alive: !remote.dead,
                protected: false,
                speed: sample.vel.length(),
            });
        }
    }
    render.missiles = game
        .missiles
        .iter()
        .map(|(id, item)| {
            let rot = Quat::from_rotation_arc(Vec3::NEG_Z, item.missile.vel.normalize_or_zero());
            (*id, (item.missile.pos, rot))
        })
        .collect();
}

pub fn score_vec(game: &OnlineGame) -> Vec<(u32, u32, u32)> {
    game.scores
        .iter()
        .map(|(id, (kills, deaths))| (*id, *kills, *deaths))
        .collect()
}

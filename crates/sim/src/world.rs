//! A whole match in one place: planes, guns, and missiles stepped
//! together. The solo mode runs this directly; tests play whole fights
//! in milliseconds.

use crate::combat::{self, Outcome};
use crate::flight::{Controls, Part, Plane};
use crate::missile::Missile;
use crate::tuning;
use glam::{Quat, Vec3};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Hit { shooter: u32, target: u32, part: Part },
    Broken { target: u32, part: Part },
    Kill { shooter: u32, target: u32 },
    Launch { id: u32, shooter: u32, target: u32 },
    Burst { id: u32, pos: Vec3 },
    Respawn { id: u32 },
}

#[derive(Default)]
pub struct World {
    pub time: f32,
    pub planes: HashMap<u32, Plane>,
    pub missiles: Vec<Missile>,
    cooldowns: HashMap<u32, f32>,
    next_missile: u32,
    next_spawn: u32,
}

impl World {
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawn a plane at a spread-out spot, protected for PROTECT_S.
    pub fn spawn(&mut self, id: u32) -> &Plane {
        self.next_spawn += 1;
        let angle = self.next_spawn as f32 * 2.4;
        let slot = (id % 16) as f32;
        let pos = Vec3::new(angle.cos() * 400.0, 200.0 + 30.0 * slot, angle.sin() * 400.0);
        let rot = Quat::from_rotation_y(angle);
        let plane = Plane::new(id, pos, rot, self.time);
        self.planes.insert(id, plane);
        &self.planes[&id]
    }

    pub fn respawn(&mut self, id: u32) {
        let hp = self.spawn(id);
        let _ = hp;
    }

    /// Step physics and respawns with the given controls.
    pub fn step(&mut self, dt: f32, controls: &HashMap<u32, Controls>) -> Vec<Event> {
        self.time += dt;
        let mut events = Vec::new();
        let ids: Vec<u32> = self.planes.keys().copied().collect();
        for id in ids {
            let respawn = {
                let plane = self.planes.get_mut(&id).unwrap();
                if plane.alive {
                    let c = controls.get(&id).copied().unwrap_or_default();
                    plane.step(dt, &c);
                    false
                } else {
                    plane.respawn_in -= dt;
                    plane.respawn_in <= 0.0
                }
            };
            if respawn {
                self.spawn(id);
                events.push(Event::Respawn { id });
            }
        }
        events
    }

    /// Fire the guns of one plane, if its cooldown allows. The shooter
    /// decides the hit and sees it at once.
    pub fn fire(&mut self, id: u32) -> Vec<Event> {
        let cooldown = self.cooldowns.entry(id).or_insert(0.0);
        if *cooldown > self.time {
            return Vec::new();
        }
        let Some(shooter) = self.planes.get(&id).cloned() else {
            return Vec::new();
        };
        if !shooter.alive {
            return Vec::new();
        }
        *cooldown = self.time + 1.0 / tuning::GUN_RATE;

        let origin = shooter.pos + shooter.forward() * 6.0;
        let dir = shooter.forward();
        let mut events = Vec::new();
        let mut best: Option<(u32, Part, f32)> = None;
        for other in self.planes.values() {
            if other.id == id || !other.alive || other.protected(self.time) {
                continue;
            }
            if let Some((part, dist)) = combat::ray_hit(origin, dir, other) {
                if best.map_or(true, |(_, _, d)| dist < d) {
                    best = Some((other.id, part, dist));
                }
            }
        }
        if let Some((target_id, part, _)) = best {
            let now = self.time;
            let target = self.planes.get_mut(&target_id).unwrap();
            match combat::apply_damage(target, part, tuning::GUN_DAMAGE, now) {
                Outcome::Damaged => events.push(Event::Hit { shooter: id, target: target_id, part }),
                Outcome::Broken => {
                    events.push(Event::Hit { shooter: id, target: target_id, part });
                    events.push(Event::Broken { target: target_id, part });
                }
                Outcome::Killed => {
                    events.push(Event::Hit { shooter: id, target: target_id, part });
                    events.push(Event::Kill { shooter: id, target: target_id });
                }
                Outcome::Protected => {}
            }
        }
        events
    }

    /// Launch a locked missile from a plane.
    pub fn launch(&mut self, shooter: u32, target: u32) -> Option<Event> {
        let plane = self.planes.get(&shooter)?;
        if !plane.alive {
            return None;
        }
        self.next_missile += 1;
        let missile = Missile::launch(self.next_missile, plane, target);
        self.missiles.push(missile);
        Some(Event::Launch { id: self.next_missile, shooter, target })
    }

    /// Step every missile, applying bursts.
    pub fn step_missiles(&mut self, dt: f32) -> Vec<Event> {
        let now = self.time;
        let mut events = Vec::new();
        let mut bursts = Vec::new();
        for missile in &mut self.missiles {
            let target = match self.planes.get(&missile.target) {
                Some(p) => p.clone(),
                None => continue,
            };
            if let Some(pos) = missile.step(dt, &target) {
                bursts.push((missile.id, pos));
            }
        }
        self.missiles.retain(|m| !m.expired());
        for (id, pos) in bursts {
            events.push(Event::Burst { id, pos });
            let ids: Vec<u32> = self.planes.keys().copied().collect();
            for pid in ids {
                let plane = self.planes.get_mut(&pid).unwrap();
                for (part, outcome) in combat::burst_damage(plane, pos, now) {
                    match outcome {
                        Outcome::Damaged => {
                            events.push(Event::Hit { shooter: 0, target: pid, part })
                        }
                        Outcome::Broken => {
                            events.push(Event::Hit { shooter: 0, target: pid, part });
                            events.push(Event::Broken { target: pid, part });
                        }
                        Outcome::Killed => {
                            events.push(Event::Kill { shooter: 0, target: pid });
                        }
                        Outcome::Protected => {}
                    }
                }
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::{self, Skill};

    #[test]
    fn a_spawn_is_protected_for_five_seconds() {
        let mut world = World::new();
        world.spawn(1);
        world.spawn(2);
        // Put plane 2 right in the sights of plane 1.
        let pos = world.planes[&1].pos + world.planes[&1].forward() * 100.0;
        world.planes.get_mut(&2).unwrap().pos = pos;
        world.planes.get_mut(&2).unwrap().vel = Vec3::ZERO;
        for _ in 0..(tuning::PROTECT_S * 60.0) as u32 - 1 {
            world.step(1.0 / 60.0, &HashMap::new());
            assert!(world.fire(1).is_empty(), "hit a protected plane");
        }
    }

    #[test]
    fn a_full_duel_produces_a_kill() {
        let mut world = World::new();
        world.spawn(1);
        world.spawn(2);
        // Age out the protection.
        world.step(tuning::PROTECT_S + 0.1, &HashMap::new());
        let mut kills = 0;
        let dt = 1.0 / 60.0;
        for _ in 0..(60.0 * 90.0) as usize {
            let mut controls = HashMap::new();
            for id in [1u32, 2] {
                let plane = world.planes[&id].clone();
                let target = bots::pick_target(&plane, world.planes.values(), world.time).cloned();
                let action = bots::fly(&plane, Skill::Veteran, target.as_ref(), world.time);
                controls.insert(id, action.controls);
                if action.fire {
                    for event in world.fire(id) {
                        if matches!(event, Event::Kill { .. }) {
                            kills += 1;
                        }
                    }
                }
            }
            world.step(dt, &controls);
            if kills > 0 {
                break;
            }
        }
        assert!(kills > 0, "two veterans produced no kill in 90 s");
    }

    #[test]
    fn respawn_comes_back_after_three_seconds() {
        let mut world = World::new();
        world.spawn(1);
        world.step(tuning::PROTECT_S + 0.1, &HashMap::new());
        world.planes.get_mut(&1).unwrap().alive = false;
        world.planes.get_mut(&1).unwrap().respawn_in = tuning::RESPAWN_S;
        let mut respawned = false;
        for _ in 0..(60.0 * 5.0) as usize {
            for event in world.step(1.0 / 60.0, &HashMap::new()) {
                if matches!(event, Event::Respawn { id: 1 }) {
                    respawned = true;
                }
            }
        }
        assert!(respawned);
        assert!(world.planes[&1].alive);
        assert!(world.planes[&1].protected(world.time));
    }

    #[test]
    fn several_machines_agree_over_an_ordered_bus() {
        use crate::lobby::{Command, Lobby};
        use crate::wire::{decode, encode, Message};

        struct Node {
            id: u32,
            plane: Plane,
            lobby: Lobby,
            bots: HashMap<u32, Plane>,
            kills: HashMap<u32, u32>,
        }

        let mut nodes: Vec<Node> = [1u32, 2, 3]
            .into_iter()
            .map(|id| {
                let mut plane = Plane::new(id, Vec3::new(0.0, 200.0, -(id as f32) * 100.0), Quat::IDENTITY, 0.0);
                plane.protect_until = 0.0;
                let mut lobby = Lobby::default();
                for m in [1u32, 2, 3] {
                    lobby.apply(m, Command::Join { num: m, name: format!("P{m}") }).unwrap();
                }
                let mut bots = HashMap::new();
                if lobby.host() == Some(id) {
                    bots.insert(100, Plane::new(100, Vec3::ZERO, Quat::IDENTITY, 0.0));
                }
                Node { id, plane, lobby, bots, kills: HashMap::new() }
            })
            .collect();

        // Ordered bus queue of wire frames: (sender, raw_bytes)
        let mut bus: Vec<(u32, Vec<u8>)> = Vec::new();
        for _ in 0..30 {
            bus.push((
                1,
                encode(&Message::Hit {
                    shooter: 1,
                    target: 2,
                    part: Part::Fuselage,
                    damage: tuning::GUN_DAMAGE,
                }),
            ));
        }

        let mut idx = 0;
        while idx < bus.len() {
            let (_from, raw) = bus[idx].clone();
            idx += 1;
            let msg = decode(&raw).unwrap();
            let mut follow_up = Vec::new();
            for node in &mut nodes {
                match msg.clone() {
                    Message::Hit { shooter, target, part, damage } => {
                        // Only the owner of the target applies damage.
                        if node.id == target && node.plane.alive {
                            if combat::apply_damage(&mut node.plane, part, damage, 10.0)
                                == Outcome::Killed
                            {
                                follow_up.push((
                                    node.id,
                                    encode(&Message::Kill { shooter, target }),
                                ));
                            }
                        }
                    }
                    Message::Kill { shooter, target: _ } => {
                        *node.kills.entry(shooter).or_default() += 1;
                    }
                    _ => {}
                }
            }
            bus.extend(follow_up);
        }

        // Only node 2 took damage, and all three nodes recorded the exact same kill count.
        assert_eq!(nodes[0].plane.parts.fuselage, tuning::FUSELAGE_HEALTH);
        assert_eq!(nodes[1].plane.parts.fuselage, 0);
        assert!(!nodes[1].plane.alive);
        assert_eq!(nodes[0].kills.get(&1), Some(&1));
        assert_eq!(nodes[0].kills, nodes[1].kills);
        assert_eq!(nodes[1].kills, nodes[2].kills);

        // Host 1 leaves: host and bot ownership pass to node 2.
        for node in &mut nodes {
            node.lobby.apply(1, Command::Leave { num: 1 }).unwrap();
            if node.lobby.host() == Some(node.id) {
                node.bots.entry(100).or_insert_with(|| Plane::new(100, Vec3::ZERO, Quat::IDENTITY, 0.0));
            } else {
                node.bots.clear();
            }
        }
        assert!(nodes[0].bots.is_empty());
        assert!(nodes[1].bots.contains_key(&100));
    }
}

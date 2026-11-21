//! The eight game messages on the bus. Encoding is hand-rolled and
//! compact: a state is 38 bytes, sent about 30 times a second per plane.

use crate::bots::Skill;
use crate::flight::{Part, Parts, Plane, PARTS};
use crate::lobby::Lobby;
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub id: u32,
    /// The sender's clock at send time, in seconds.
    pub clock: f64,
    pub pos: Vec3,
    pub vel: Vec3,
    pub rot: Quat,
    pub parts: Parts,
    pub throttle: f32,
}

impl State {
    pub fn of(plane: &Plane, clock: f64) -> Self {
        Self {
            id: plane.id,
            clock,
            pos: plane.pos,
            vel: plane.vel,
            rot: plane.rot(),
            parts: plane.parts,
            throttle: plane.throttle,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    State(State),
    Hit { shooter: u32, target: u32, part: Part, damage: u8 },
    Kill { shooter: u32, target: u32 },
    Launch { id: u32, shooter: u32, target: u32, pos: Vec3, dir: Vec3 },
    Burst { id: u32, pos: Vec3 },
    EndMatch { scores: Vec<(u32, u32, u32)> },
    Lobby(Lobby),
    Profile { id: u32, name: String },
}

const T_STATE: u8 = 0;
const T_HIT: u8 = 1;
const T_KILL: u8 = 2;
const T_LAUNCH: u8 = 3;
const T_BURST: u8 = 4;
const T_END: u8 = 5;
const T_LOBBY: u8 = 6;
const T_PROFILE: u8 = 7;

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_f32(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_f64(out: &mut Vec<u8>, v: f64) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_vec3(out: &mut Vec<u8>, v: Vec3) {
    put_f32(out, v.x);
    put_f32(out, v.y);
    put_f32(out, v.z);
}

/// A quaternion as four i16: eight bytes instead of sixteen.
fn put_quat(out: &mut Vec<u8>, q: Quat) {
    let q = q.normalize();
    for v in [q.x, q.y, q.z, q.w] {
        out.extend_from_slice(&((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
}

fn part_tag(part: Part) -> u8 {
    PARTS.iter().position(|p| *p == part).unwrap() as u8
}

fn tag_part(tag: u8) -> Option<Part> {
    PARTS.get(tag as usize).copied()
}

pub fn encode(msg: &Message) -> Vec<u8> {
    let mut out = Vec::new();
    match msg {
        Message::State(s) => {
            out.push(T_STATE);
            put_u32(&mut out, s.id);
            put_f64(&mut out, s.clock);
            put_vec3(&mut out, s.pos);
            put_vec3(&mut out, s.vel);
            put_quat(&mut out, s.rot);
            out.push(s.parts.mask());
            out.push((s.throttle.clamp(0.0, 1.0) * 255.0) as u8);
        }
        Message::Hit { shooter, target, part, damage } => {
            out.push(T_HIT);
            put_u32(&mut out, *shooter);
            put_u32(&mut out, *target);
            out.push(part_tag(*part));
            out.push(*damage);
        }
        Message::Kill { shooter, target } => {
            out.push(T_KILL);
            put_u32(&mut out, *shooter);
            put_u32(&mut out, *target);
        }
        Message::Launch { id, shooter, target, pos, dir } => {
            out.push(T_LAUNCH);
            put_u32(&mut out, *id);
            put_u32(&mut out, *shooter);
            put_u32(&mut out, *target);
            put_vec3(&mut out, *pos);
            put_vec3(&mut out, *dir);
        }
        Message::Burst { id, pos } => {
            out.push(T_BURST);
            put_u32(&mut out, *id);
            put_vec3(&mut out, *pos);
        }
        Message::EndMatch { scores } => {
            out.push(T_END);
            out.push(scores.len() as u8);
            for (id, kills, deaths) in scores {
                put_u32(&mut out, *id);
                put_u32(&mut out, *kills);
                put_u32(&mut out, *deaths);
            }
        }
        Message::Lobby(lobby) => {
            out.push(T_LOBBY);
            out.push(lobby.bots);
            out.push(SKILLS_WIRE.iter().position(|s| *s == lobby.skill).unwrap_or(1) as u8);
            out.push(lobby.running as u8);
            out.push(lobby.members.len() as u8);
            for m in &lobby.members {
                put_u32(&mut out, m.num);
                let bytes = m.name.as_bytes();
                out.push(bytes.len().min(255) as u8);
                out.extend_from_slice(&bytes[..bytes.len().min(255)]);
            }
        }
        Message::Profile { id, name } => {
            out.push(T_PROFILE);
            put_u32(&mut out, *id);
            let bytes = name.as_bytes();
            out.push(bytes.len().min(255) as u8);
            out.extend_from_slice(&bytes[..bytes.len().min(255)]);
        }
    }
    out
}

const SKILLS_WIRE: [Skill; 4] = [Skill::Rookie, Skill::Pilot, Skill::Veteran, Skill::Ace];

struct Read<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Read<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let slice = self.bytes.get(self.at..self.at + n)?;
        self.at += n;
        Some(slice)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    }
    fn f32(&mut self) -> Option<f32> {
        self.take(4).map(|b| f32::from_le_bytes(b.try_into().unwrap()))
    }
    fn f64(&mut self) -> Option<f64> {
        self.take(8).map(|b| f64::from_le_bytes(b.try_into().unwrap()))
    }
    fn vec3(&mut self) -> Option<Vec3> {
        Some(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
    fn quat(&mut self) -> Option<Quat> {
        let mut v = [0f32; 4];
        for slot in &mut v {
            let i = i16::from_le_bytes(self.take(2)?.try_into().unwrap());
            *slot = i as f32 / 32767.0;
        }
        Some(Quat::from_xyzw(v[0], v[1], v[2], v[3]).normalize())
    }
    fn string(&mut self) -> Option<String> {
        let len = self.u8()? as usize;
        String::from_utf8(self.take(len)?.to_vec()).ok()
    }
}

pub fn decode(bytes: &[u8]) -> Option<Message> {
    let mut r = Read { bytes, at: 0 };
    match r.u8()? {
        T_STATE => Some(Message::State(State {
            id: r.u32()?,
            clock: r.f64()?,
            pos: r.vec3()?,
            vel: r.vec3()?,
            rot: r.quat()?,
            parts: Parts::from_mask(r.u8()?),
            throttle: r.u8()? as f32 / 255.0,
        })),
        T_HIT => Some(Message::Hit {
            shooter: r.u32()?,
            target: r.u32()?,
            part: tag_part(r.u8()?)?,
            damage: r.u8()?,
        }),
        T_KILL => Some(Message::Kill { shooter: r.u32()?, target: r.u32()? }),
        T_LAUNCH => Some(Message::Launch {
            id: r.u32()?,
            shooter: r.u32()?,
            target: r.u32()?,
            pos: r.vec3()?,
            dir: r.vec3()?,
        }),
        T_BURST => Some(Message::Burst { id: r.u32()?, pos: r.vec3()? }),
        T_END => {
            let n = r.u8()?;
            let mut scores = Vec::with_capacity(n as usize);
            for _ in 0..n {
                scores.push((r.u32()?, r.u32()?, r.u32()?));
            }
            Some(Message::EndMatch { scores })
        }
        T_LOBBY => {
            let bots = r.u8()?;
            let skill = SKILLS_WIRE.get(r.u8()? as usize).copied().unwrap_or(Skill::Pilot);
            let running = r.u8()? != 0;
            let n = r.u8()?;
            let mut members = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let num = r.u32()?;
                let name = r.string()?;
                members.push(crate::lobby::Member { num, name });
            }
            Some(Message::Lobby(Lobby { members, bots, skill, running }))
        }
        T_PROFILE => Some(Message::Profile { id: r.u32()?, name: r.string()? }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lobby::Member;

    fn round_trip(msg: Message) {
        let bytes = encode(&msg);
        let back = decode(&bytes).expect("decode");
        assert_eq!(back, msg, "round trip failed");
    }

    #[test]
    fn a_state_is_about_forty_bytes() {
        let plane = Plane::new(3, Vec3::new(1.0, 2.0, 3.0), Quat::IDENTITY, 0.0);
        let bytes = encode(&Message::State(State::of(&plane, 12.5)));
        assert_eq!(bytes.len(), 47, "1 tag + 4 id + 8 clock + 24 + 8 + 2");
    }

    #[test]
    fn round_trips() {
        let plane = Plane::new(
            7,
            Vec3::new(100.5, -20.0, 33.0),
            Quat::from_rotation_y(1.1),
            0.0,
        );
        // The quaternion rides in four i16, so compare it loosely.
        let state = Message::State(State::of(&plane, 9.25));
        let back = decode(&encode(&state)).expect("decode");
        match (back, &state) {
            (Message::State(a), Message::State(b)) => {
                assert_eq!(a.id, b.id);
                assert_eq!(a.clock, b.clock);
                assert_eq!(a.pos, b.pos);
                assert!((a.rot.dot(b.rot)).abs() > 0.99999, "{} vs {}", a.rot, b.rot);
            }
            _ => panic!("wrong message"),
        }
        round_trip(Message::Hit { shooter: 1, target: 2, part: Part::LeftWing, damage: 4 });
        round_trip(Message::Kill { shooter: 9, target: 3 });
        round_trip(Message::Launch {
            id: 5,
            shooter: 1,
            target: 2,
            pos: Vec3::new(1.0, 0.0, -6.0),
            dir: Vec3::NEG_Z,
        });
        round_trip(Message::Burst { id: 5, pos: Vec3::new(9.0, 1.0, -30.0) });
        round_trip(Message::EndMatch { scores: vec![(1, 3, 1), (2, 0, 4)] });
        round_trip(Message::Profile { id: 42, name: "José Müller".into() });
    }

    #[test]
    fn a_lobby_round_trip_keeps_utf8_names() {
        let lobby = Lobby {
            members: vec![
                Member { num: 1, name: "Renée".into() },
                Member { num: 2, name: " pilot 2 ".into() },
            ],
            bots: 3,
            skill: Skill::Ace,
            running: true,
        };
        round_trip(Message::Lobby(lobby));
    }

    #[test]
    fn truncated_input_decodes_to_none() {
        let bytes = encode(&Message::Kill { shooter: 1, target: 2 });
        for cut in 0..bytes.len() {
            assert!(decode(&bytes[..cut]).is_none(), "cut at {cut}");
        }
    }
}

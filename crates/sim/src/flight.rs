use crate::tuning;
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Controls {
    /// Nose up is positive, in -1..=1.
    pub pitch: f32,
    /// Right is positive, in -1..=1.
    pub yaw: f32,
    /// Right wing down is positive, in -1..=1.
    pub roll: f32,
    /// 0..=1, the target speed between MIN_SPEED and MAX_SPEED.
    pub throttle: f32,
    pub boost: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Part {
    LeftWing,
    RightWing,
    Tail,
    Engine,
    Fuselage,
}

pub const PARTS: [Part; 5] = [
    Part::LeftWing,
    Part::RightWing,
    Part::Tail,
    Part::Engine,
    Part::Fuselage,
];

impl Part {
    pub fn offset(self) -> Vec3 {
        match self {
            Part::LeftWing => Vec3::new(-6.0, 0.0, 0.0),
            Part::RightWing => Vec3::new(6.0, 0.0, 0.0),
            Part::Tail => Vec3::new(0.0, 1.0, 8.0),
            Part::Engine => Vec3::new(0.0, 0.0, 6.0),
            Part::Fuselage => Vec3::ZERO,
        }
    }

    pub fn radius(self) -> f32 {
        match self {
            Part::Fuselage => 3.0,
            _ => tuning::HIT_SPHERE,
        }
    }

    pub fn max_health(self) -> u8 {
        match self {
            Part::LeftWing | Part::RightWing => tuning::WING_HEALTH,
            Part::Tail => tuning::TAIL_HEALTH,
            Part::Engine => tuning::ENGINE_HEALTH,
            Part::Fuselage => tuning::FUSELAGE_HEALTH,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Parts {
    pub left_wing: u8,
    pub right_wing: u8,
    pub tail: u8,
    pub engine: u8,
    pub fuselage: u8,
}

impl Default for Parts {
    fn default() -> Self {
        Self {
            left_wing: tuning::WING_HEALTH,
            right_wing: tuning::WING_HEALTH,
            tail: tuning::TAIL_HEALTH,
            engine: tuning::ENGINE_HEALTH,
            fuselage: tuning::FUSELAGE_HEALTH,
        }
    }
}

impl Parts {
    pub fn health(&self, part: Part) -> u8 {
        match part {
            Part::LeftWing => self.left_wing,
            Part::RightWing => self.right_wing,
            Part::Tail => self.tail,
            Part::Engine => self.engine,
            Part::Fuselage => self.fuselage,
        }
    }

    pub fn damage(&mut self, part: Part, amount: u8) {
        let field = match part {
            Part::LeftWing => &mut self.left_wing,
            Part::RightWing => &mut self.right_wing,
            Part::Tail => &mut self.tail,
            Part::Engine => &mut self.engine,
            Part::Fuselage => &mut self.fuselage,
        };
        *field = field.saturating_sub(amount);
    }

    /// A bitmask of the parts still attached, bit i = PARTS[i].
    pub fn mask(&self) -> u8 {
        let mut mask = 0;
        for (i, part) in PARTS.iter().enumerate() {
            if self.health(*part) > 0 {
                mask |= 1 << i;
            }
        }
        mask
    }

    pub fn from_mask(mask: u8) -> Self {
        let mut parts = Parts::default();
        for (i, part) in PARTS.iter().enumerate() {
            if mask & (1 << i) == 0 {
                parts.damage(*part, part.max_health());
            }
        }
        parts
    }
}

/// Pitch and yaw commands that turn the plane toward `aim`.
/// The mouse cursor and the bots both produce aim points, so flight
/// and camera stay one path.
pub fn steer_toward(plane: &Plane, aim: Vec3) -> (f32, f32) {
    let to = aim - plane.pos;
    if to.length_squared() < 1e-6 {
        return (0.0, 0.0);
    }
    let to = to.normalize();
    let want_heading = to.x.atan2(-to.z);
    let want_pitch = to.y.asin();
    let yaw = (wrap_angle(want_heading - plane.heading) * tuning::STEER_GAIN).clamp(-1.0, 1.0);
    let pitch = ((want_pitch - plane.pitch) * tuning::STEER_GAIN).clamp(-1.0, 1.0);
    (pitch, yaw)
}

/// The shortest signed angle from 0 to `a`, in -pi..=pi.
pub fn wrap_angle(a: f32) -> f32 {
    use std::f32::consts::PI;
    (a + PI).rem_euclid(2.0 * PI) - PI
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plane {
    pub id: u32,
    pub pos: Vec3,
    pub vel: Vec3,
    /// Radians around the world up axis; 0 faces -Z, positive is right.
    pub heading: f32,
    /// Radians above the horizon, in -1.4..=1.4.
    pub pitch: f32,
    /// Radians of bank, positive with the right wing down.
    pub roll: f32,
    pub throttle: f32,
    pub parts: Parts,
    pub alive: bool,
    pub respawn_in: f32,
    pub protect_until: f32,
}

impl Plane {
    pub fn new(id: u32, pos: Vec3, rot: Quat, now: f32) -> Self {
        let fwd = rot * Vec3::NEG_Z;
        Self {
            id,
            pos,
            vel: fwd * tuning::MIN_SPEED,
            heading: fwd.x.atan2(-fwd.z),
            pitch: fwd.y.clamp(-1.0, 1.0).asin(),
            roll: 0.0,
            throttle: 1.0,
            parts: Parts::default(),
            alive: true,
            respawn_in: 0.0,
            protect_until: now + tuning::PROTECT_S,
        }
    }

    pub fn rot(&self) -> Quat {
        Quat::from_rotation_y(self.heading)
            * Quat::from_rotation_x(self.pitch)
            * Quat::from_rotation_z(-self.roll)
    }

    pub fn forward(&self) -> Vec3 {
        let (sin, cos) = self.heading.sin_cos();
        Vec3::new(sin * self.pitch.cos(), self.pitch.sin(), -cos * self.pitch.cos())
    }

    pub fn speed(&self) -> f32 {
        self.vel.length()
    }

    pub fn protected(&self, now: f32) -> bool {
        now < self.protect_until
    }

    pub fn part_center(&self, part: Part) -> Vec3 {
        self.pos + self.rot() * part.offset()
    }

    pub fn step(&mut self, dt: f32, c: &Controls) {
        if !self.alive {
            return;
        }
        self.throttle = c.throttle.clamp(0.0, 1.0);
        let authority = if self.parts.tail == 0 {
            tuning::TAIL_AUTHORITY
        } else {
            1.0
        };

        // A lost wing pulls the plane toward the wound: it rolls and
        // turns until the pilot counter-steers.
        let mut yaw_cmd = c.yaw;
        let mut roll_spin = 0.0;
        if self.parts.left_wing == 0 {
            yaw_cmd -= tuning::WING_PULL;
            roll_spin -= tuning::WING_ROLL;
        }
        if self.parts.right_wing == 0 {
            yaw_cmd += tuning::WING_PULL;
            roll_spin += tuning::WING_ROLL;
        }

        self.heading = wrap_angle(
            self.heading + yaw_cmd.clamp(-1.0, 1.0) * tuning::YAW_RATE * authority * dt,
        );
        self.pitch = (self.pitch + c.pitch.clamp(-1.0, 1.0) * tuning::PITCH_RATE * authority * dt)
            .clamp(-tuning::MAX_PITCH, tuning::MAX_PITCH);

        // Bank into the turn; level off otherwise; spin with a lost wing.
        if roll_spin != 0.0 {
            self.roll = wrap_angle(self.roll + roll_spin * dt);
        } else {
            let want = if c.roll.abs() > 0.01 {
                c.roll * tuning::MAX_BANK
            } else {
                -c.yaw * tuning::BANK_ROLL
            };
            let step = tuning::ROLL_RATE * dt;
            self.roll += (want - self.roll).clamp(-step, step);
        }

        let engine_ok = self.parts.engine > 0;
        let mut speed = self.speed();
        if engine_ok {
            let mut target = tuning::MIN_SPEED
                + self.throttle * (tuning::MAX_SPEED - tuning::MIN_SPEED);
            if c.boost {
                target *= tuning::BOOST_MULT;
            }
            speed += (target - speed).clamp(-tuning::ACCEL * dt, tuning::ACCEL * dt);
        } else {
            // A dead engine glides and sinks.
            speed = (speed - tuning::DEAD_ENGINE_DRAG * dt).max(tuning::MIN_SPEED * 0.5);
            self.pitch = (self.pitch - tuning::DEAD_ENGINE_SINK * dt)
                .clamp(-tuning::MAX_PITCH, tuning::MAX_PITCH);
        }
        self.vel = self.forward() * speed;
        self.pos += self.vel * dt;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cruise() -> Plane {
        Plane::new(1, Vec3::ZERO, Quat::IDENTITY, 0.0)
    }

    fn full() -> Controls {
        Controls { throttle: 1.0, ..Default::default() }
    }

    fn step_for(p: &mut Plane, c: &Controls, seconds: f32) {
        let dt = 1.0 / 60.0;
        for _ in 0..(seconds * 60.0) as usize {
            p.step(dt, c);
        }
    }

    #[test]
    fn straight_level_flight_keeps_heading_and_speed() {
        let mut p = cruise();
        step_for(&mut p, &full(), 5.0);
        let fwd = p.forward();
        assert!(fwd.dot(Vec3::NEG_Z) > 0.999, "heading drifted: {fwd}");
        assert!((p.speed() - tuning::MAX_SPEED).abs() < 1.0);
        assert!(p.pos.z < -500.0);
    }

    #[test]
    fn speed_does_not_depend_on_step_size() {
        let c = Controls { throttle: 0.0, ..Default::default() };
        let mut coarse = cruise();
        let mut fine = cruise();
        for _ in 0..60 {
            coarse.step(0.1, &c);
        }
        for _ in 0..600 {
            fine.step(0.01, &c);
        }
        assert!((coarse.speed() - fine.speed()).abs() < 1.0);
        assert!((coarse.pos - fine.pos).length() < 5.0);
    }

    #[test]
    fn steer_toward_turns_onto_the_aim() {
        let mut p = cruise();
        let aim = Vec3::new(5000.0, 0.0, -5000.0);
        let dt = 1.0 / 60.0;
        for _ in 0..600 {
            let (pitch, yaw) = steer_toward(&p, aim);
            p.step(dt, &Controls { pitch, yaw, ..full() });
        }
        let to = (aim - p.pos).normalize();
        assert!(
            p.forward().dot(to) > 0.99,
            "did not reach the aim: fwd {} to {to}",
            p.forward()
        );
    }

    #[test]
    fn yaw_right_turns_right() {
        let mut p = cruise();
        step_for(&mut p, &Controls { yaw: 1.0, ..full() }, 1.0);
        assert!(p.forward().x > 0.2, "turned left: {}", p.forward());
    }

    #[test]
    fn wings_level_after_a_turn() {
        let mut p = cruise();
        step_for(&mut p, &Controls { yaw: 1.0, ..full() }, 2.0);
        step_for(&mut p, &full(), 3.0);
        assert!(p.roll.abs() < 0.05, "still banked: {}", p.roll);
    }

    #[test]
    fn a_lost_wing_rolls_and_pulls_the_plane() {
        let mut left = cruise();
        left.parts.damage(Part::LeftWing, tuning::WING_HEALTH);
        let heading0 = left.heading;
        step_for(&mut left, &full(), 2.0);
        assert!(left.roll < -0.5, "no roll: {}", left.roll);
        assert!(
            wrap_angle(left.heading - heading0) < -0.1,
            "no pull: {}",
            left.heading
        );

        let mut right = cruise();
        right.parts.damage(Part::RightWing, tuning::WING_HEALTH);
        step_for(&mut right, &full(), 2.0);
        assert!(right.roll > 0.5, "no roll: {}", right.roll);
    }

    #[test]
    fn a_dead_engine_bleeds_speed_and_sinks() {
        let mut p = cruise();
        step_for(&mut p, &full(), 2.0);
        let before = p.speed();
        p.parts.damage(Part::Engine, tuning::ENGINE_HEALTH);
        step_for(&mut p, &full(), 3.0);
        assert!(p.speed() < before - 30.0, "kept speed: {}", p.speed());
        assert!(p.pitch < 0.0, "did not sink: {}", p.pitch);
    }

    #[test]
    fn a_lost_tail_weakens_control() {
        let turn = |tail_hp: u8| {
            let mut p = cruise();
            p.parts.damage(Part::Tail, tuning::TAIL_HEALTH - tail_hp);
            step_for(&mut p, &Controls { yaw: 1.0, ..full() }, 2.0);
            p.heading.abs()
        };
        let full_turn = turn(tuning::TAIL_HEALTH);
        let weak = turn(0);
        assert!(weak < full_turn * 0.7, "tailless turned as well: {weak} vs {full_turn}");
    }

    #[test]
    fn parts_mask_round_trip() {
        let mut parts = Parts::default();
        parts.damage(Part::LeftWing, tuning::WING_HEALTH);
        parts.damage(Part::Engine, 10);
        let restored = Parts::from_mask(parts.mask());
        assert_eq!(restored.left_wing, 0);
        assert_eq!(restored.right_wing, tuning::WING_HEALTH);
        assert_eq!(restored.engine, tuning::ENGINE_HEALTH);
        // A fully broken engine drops out of the mask.
        parts.damage(Part::Engine, tuning::ENGINE_HEALTH);
        assert_eq!(Parts::from_mask(parts.mask()).engine, 0);
    }
}

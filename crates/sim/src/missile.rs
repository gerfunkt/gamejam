use crate::flight::Plane;
use crate::tuning;
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

/// A launch needs the target held in the lock cone for LOCK_TIME.
#[derive(Clone, Debug, Default)]
pub struct Lock {
    pub target: Option<u32>,
    pub held: f32,
}

impl Lock {
    pub fn locked(&self) -> Option<u32> {
        if self.held >= tuning::LOCK_TIME {
            self.target
        } else {
            None
        }
    }

    /// Track the best candidate: alive, unprotected, in the cone, in range.
    pub fn update<'a>(
        &mut self,
        dt: f32,
        shooter: &Plane,
        candidates: impl Iterator<Item = &'a Plane>,
        now: f32,
    ) {
        let fwd = shooter.forward();
        let best = candidates
            .filter(|p| p.alive && !p.protected(now))
            .filter(|p| {
                let to = p.pos - shooter.pos;
                let dist = to.length();
                dist < tuning::LOCK_RANGE && fwd.angle_between(to / dist) <= tuning::LOCK_CONE
            })
            .min_by(|a, b| {
                (a.pos - shooter.pos)
                    .length_squared()
                    .total_cmp(&(b.pos - shooter.pos).length_squared())
            });
        match best {
            Some(p) if Some(p.id) == self.target => {
                self.held = (self.held + dt).min(tuning::LOCK_TIME * 2.0);
            }
            Some(p) => {
                self.target = Some(p.id);
                self.held = 0.0;
            }
            None => {
                self.target = None;
                self.held = 0.0;
            }
        }
    }

    pub fn reset(&mut self) {
        self.target = None;
        self.held = 0.0;
    }
}

/// Rotate `from` toward `to` by at most `max` radians.
pub fn rotate_toward(from: Vec3, to: Vec3, max: f32) -> Vec3 {
    let angle = from.angle_between(to);
    if angle <= max || angle < 1e-6 {
        return to;
    }
    Quat::from_axis_angle(from.cross(to).normalize(), max) * from
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Missile {
    pub id: u32,
    pub shooter: u32,
    pub target: u32,
    pub pos: Vec3,
    pub vel: Vec3,
    pub fuel: f32,
    pub grace: f32,
    /// The seeker lost sight of the target and flies straight on.
    pub blind: bool,
}

impl Missile {
    pub fn launch(id: u32, shooter: &Plane, target: u32) -> Self {
        Self {
            id,
            shooter: shooter.id,
            target,
            pos: shooter.pos + shooter.forward() * 6.0,
            vel: shooter.forward() * tuning::MISSILE_SPEED,
            fuel: tuning::MISSILE_FUEL,
            grace: tuning::MISSILE_GRACE,
            blind: false,
        }
    }

    pub fn expired(&self) -> bool {
        self.fuel <= 0.0 && self.grace <= 0.0
    }

    /// Step the missile. Returns the burst position when it reaches its target.
    pub fn step(&mut self, dt: f32, target: &Plane) -> Option<Vec3> {
        if self.expired() {
            return None;
        }
        if self.fuel > 0.0 {
            self.fuel -= dt;
            let to = target.pos - self.pos;
            let dist = to.length();
            if dist <= tuning::BURST_RADIUS {
                return Some(self.pos);
            }
            let dir = self.vel.normalize();
            if self.blind || dir.angle_between(to / dist) > tuning::MISSILE_CONE {
                // A missile that loses sight of its target, because the
                // target broke hard across its nose, flies straight on
                // and never looks again.
                self.blind = true;
            } else {
                // The seeker leads the target.
                let t_go = dist / tuning::MISSILE_SPEED;
                let aim = target.pos + target.vel * t_go;
                let want = (aim - self.pos).normalize();
                let new_dir = rotate_toward(dir, want, tuning::MISSILE_TURN * dt);
                self.vel = new_dir * tuning::MISSILE_SPEED;
            }
        } else {
            self.grace -= dt;
            // Burnt out: drag kills the speed fast.
            let speed = (self.vel.length() - tuning::MISSILE_DRAG * dt).max(0.0);
            self.vel = self.vel.normalize_or_zero() * speed;
        }
        self.pos += self.vel * dt;
        if target.alive && (target.pos - self.pos).length() <= tuning::BURST_RADIUS {
            return Some(self.pos);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flight::Controls;

    /// Play a head-on pass: the target flies at the missile, and breaks
    /// hard when the missile is `break_at` metres away. Returns the
    /// burst position, if the missile caught it.
    fn head_on(break_at: f32) -> Option<Vec3> {
        let shooter_pos = Vec3::new(0.0, 0.0, 1000.0);
        let mut shooter = Plane::new(1, shooter_pos, Quat::IDENTITY, 0.0);
        shooter.protect_until = 0.0;
        // The target flies toward the shooter (toward +Z).
        let mut target = Plane::new(
            2,
            Vec3::new(0.0, 0.0, -1000.0),
            Quat::from_rotation_y(std::f32::consts::PI),
            0.0,
        );
        target.protect_until = 0.0;
        let mut missile = Missile::launch(1, &shooter, 2);
        let dt = 1.0 / 120.0;
        for _ in 0..(20.0 / dt) as usize {
            let dist = (missile.pos - target.pos).length();
            let yaw = if dist <= break_at { 1.0 } else { 0.0 };
            target.step(
                dt,
                &Controls { yaw, throttle: 1.0, boost: true, ..Default::default() },
            );
            if let Some(burst) = missile.step(dt, &target) {
                return Some(burst);
            }
            if missile.expired() {
                return None;
            }
        }
        None
    }

    #[test]
    fn a_straight_target_is_hit() {
        assert!(head_on(0.0).is_some());
    }

    #[test]
    fn an_early_break_escapes_a_head_on_missile() {
        assert_eq!(head_on(300.0), None);
    }

    #[test]
    fn a_late_break_does_not_escape() {
        assert!(head_on(100.0).is_some());
    }

    #[test]
    fn a_break_across_the_nose_escapes() {
        // Head-on: at 300 m the target turns across the missile's nose
        // and the missile cannot bend onto the new path in time.
        let mut shooter = Plane::new(1, Vec3::new(0.0, 0.0, 1000.0), Quat::IDENTITY, 0.0);
        shooter.protect_until = 0.0;
        let mut target = Plane::new(
            2,
            Vec3::new(0.0, 0.0, -1000.0),
            Quat::from_rotation_y(std::f32::consts::PI),
            0.0,
        );
        target.protect_until = 0.0;
        let mut missile = Missile::launch(1, &shooter, 2);
        let dt = 1.0 / 120.0;
        let mut burst = None;
        let mut breaking = false;
        let mut broken = 0.0;
        for _ in 0..(10.0 / dt) as usize {
            let dist = (missile.pos - target.pos).length();
            if dist <= 300.0 {
                breaking = true;
            }
            let yaw = if breaking && broken < 1.6 {
                broken += dt * tuning::YAW_RATE;
                1.0
            } else {
                0.0
            };
            target.step(dt, &Controls { yaw, ..Default::default() });
            if let Some(b) = missile.step(dt, &target) {
                burst = Some(b);
                break;
            }
            if missile.expired() {
                break;
            }
        }
        assert!(breaking, "the missile never closed in");
        assert_eq!(burst, None, "a break across the nose should escape");
    }

    #[test]
    fn a_boosting_plane_outruns_a_tail_shot() {
        // Fired from 500 m behind; the target boosts away and the
        // missile's fuel ends first.
        let mut shooter = Plane::new(1, Vec3::new(0.0, 0.0, 500.0), Quat::IDENTITY, 0.0);
        shooter.protect_until = 0.0;
        let mut target = Plane::new(2, Vec3::ZERO, Quat::IDENTITY, 0.0);
        target.protect_until = 0.0;
        let mut missile = Missile::launch(1, &shooter, 2);
        let dt = 1.0 / 120.0;
        let mut burst = None;
        for _ in 0..(10.0 / dt) as usize {
            target.step(
                dt,
                &Controls { throttle: 1.0, boost: true, ..Default::default() },
            );
            if let Some(b) = missile.step(dt, &target) {
                burst = Some(b);
                break;
            }
            if missile.expired() {
                break;
            }
        }
        assert_eq!(burst, None, "the boost should outrun the missile");
    }

    #[test]
    fn a_lock_needs_a_held_target() {
        let shooter = Plane::new(1, Vec3::ZERO, Quat::IDENTITY, 0.0);
        let mut target = Plane::new(2, Vec3::new(0.0, 0.0, -300.0), Quat::IDENTITY, 0.0);
        target.protect_until = 0.0;
        let planes = [target.clone()];
        let mut lock = Lock::default();
        let dt = 1.0 / 60.0;
        lock.update(dt, &shooter, planes.iter(), 10.0);
        assert_eq!(lock.locked(), None);
        for _ in 0..80 {
            lock.update(dt, &shooter, planes.iter(), 10.0);
        }
        assert_eq!(lock.locked(), Some(2));
    }

    #[test]
    fn a_lock_drops_with_the_target() {
        let shooter = Plane::new(1, Vec3::ZERO, Quat::IDENTITY, 0.0);
        let mut near = Plane::new(2, Vec3::new(0.0, 0.0, -300.0), Quat::IDENTITY, 0.0);
        near.protect_until = 0.0;
        let mut lock = Lock { target: Some(2), held: tuning::LOCK_TIME };
        // The target moved out of the cone.
        near.pos = Vec3::new(300.0, 0.0, -10.0);
        let planes = [near];
        lock.update(1.0 / 60.0, &shooter, planes.iter(), 10.0);
        assert_eq!(lock.locked(), None);
        assert_eq!(lock.target, None);
    }

    #[test]
    fn a_lock_ignores_protected_planes() {
        let shooter = Plane::new(1, Vec3::ZERO, Quat::IDENTITY, 0.0);
        let target = Plane::new(2, Vec3::new(0.0, 0.0, -300.0), Quat::IDENTITY, 0.0);
        let planes = [target];
        let mut lock = Lock::default();
        for _ in 0..120 {
            lock.update(1.0 / 60.0, &shooter, planes.iter(), 0.0);
        }
        assert_eq!(lock.locked(), None, "protected planes give no lock");
    }
}

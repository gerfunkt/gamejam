use crate::flight::{steer_toward, Controls, Plane};
use crate::tuning;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Skill {
    Rookie,
    Pilot,
    Veteran,
    Ace,
}

pub const SKILLS: [Skill; 4] = [Skill::Rookie, Skill::Pilot, Skill::Veteran, Skill::Ace];

impl Skill {
    pub fn name(self) -> &'static str {
        match self {
            Skill::Rookie => "Rookie",
            Skill::Pilot => "Pilot",
            Skill::Veteran => "Veteran",
            Skill::Ace => "Ace",
        }
    }

    /// How far ahead of its target the bot aims, in seconds of flight.
    fn lead(self) -> f32 {
        match self {
            Skill::Rookie => 0.3,
            Skill::Pilot => 0.6,
            Skill::Veteran => 0.9,
            Skill::Ace => 1.2,
        }
    }

    /// Share of the full steering the bot uses; below 1 it turns softer.
    fn turn(self) -> f32 {
        match self {
            Skill::Rookie => 0.6,
            Skill::Pilot => 0.8,
            Skill::Veteran => 0.9,
            Skill::Ace => 1.0,
        }
    }

    /// The firing cone, in radians off the nose.
    pub fn cone(self) -> f32 {
        match self {
            Skill::Rookie => 0.25,
            Skill::Pilot => 0.15,
            Skill::Veteran => 0.10,
            Skill::Ace => 0.06,
        }
    }

    /// The farthest distance for a missile launch. A Rookie carries none.
    pub fn missile_range(self) -> Option<f32> {
        match self {
            Skill::Rookie => None,
            Skill::Pilot => Some(300.0),
            Skill::Veteran => Some(450.0),
            Skill::Ace => Some(600.0),
        }
    }
}

/// What a bot decided this step.
#[derive(Clone, Copy, Debug, Default)]
pub struct BotAction {
    pub controls: Controls,
    pub fire: bool,
    pub wants_launch: bool,
}

/// The nearest living enemy that is not under spawn protection.
pub fn pick_target<'a>(
    me: &Plane,
    planes: impl Iterator<Item = &'a Plane>,
    now: f32,
) -> Option<&'a Plane> {
    planes
        .filter(|p| p.id != me.id && p.alive && !p.protected(now))
        .min_by(|a, b| {
            (a.pos - me.pos)
                .length_squared()
                .total_cmp(&(b.pos - me.pos).length_squared())
        })
}

pub fn fly(me: &Plane, skill: Skill, target: Option<&Plane>, now: f32) -> BotAction {
    let mut action = BotAction::default();
    let Some(target) = target else {
        // Nobody to fight: patrol near the arena centre and keep the sky busy.
        action.controls = Controls { throttle: 0.7, ..Default::default() };
        let center = glam::Vec3::new(0.0, 220.0, 0.0);
        let aim = if (center - me.pos).length() > 180.0 {
            center
        } else {
            me.pos + me.forward() * 100.0
        };
        let (pitch, yaw) = steer_toward(me, aim);
        action.controls.pitch = pitch;
        action.controls.yaw = yaw;
        return action;
    };

    let to = target.pos - me.pos;
    let dist = to.length();
    let aim = target.pos + target.vel * skill.lead() * (dist / 400.0);
    let (pitch, yaw) = steer_toward(me, aim);
    let turn = skill.turn();
    action.controls.pitch = pitch * turn;
    action.controls.yaw = yaw * turn;
    // Match the target's speed at close range so the aim settles.
    action.controls.throttle = if dist < 150.0 {
        ((target.speed() - tuning::MIN_SPEED) / (tuning::MAX_SPEED - tuning::MIN_SPEED))
            .clamp(0.0, 1.0)
    } else {
        1.0
    };
    action.controls.boost = dist > 800.0;

    let off = me.forward().angle_between(to / dist);
    action.fire = off < skill.cone() && dist < 500.0 && !target.protected(now);
    action.wants_launch = dist < 800.0;
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;
    use glam::Vec3;

    fn unprotected(mut p: Plane) -> Plane {
        p.protect_until = 0.0;
        p
    }

    /// A bot chases a slow target in a gentle turn for `seconds`;
    /// returns the gun hits it scored at 10 shots a second.
    fn duel(skill: Skill, seconds: f32) -> u32 {
        let mut bot = unprotected(Plane::new(
            1,
            Vec3::new(0.0, 0.0, 400.0),
            Quat::IDENTITY,
            0.0,
        ));
        let mut target = unprotected(Plane::new(2, Vec3::ZERO, Quat::IDENTITY, 0.0));
        target.throttle = 0.2;
        let dt = 1.0 / 60.0;
        let mut hits = 0;
        let mut cooldown = 0.0;
        for _ in 0..(seconds * 60.0) as usize {
            let action = fly(&bot, skill, Some(&target), 10.0);
            bot.step(dt, &action.controls);
            target.step(
                dt,
                &Controls { yaw: 0.2, throttle: 0.2, ..Default::default() },
            );
            cooldown -= dt;
            if action.fire && cooldown <= 0.0 {
                cooldown = 1.0 / tuning::GUN_RATE;
                if crate::combat::ray_hit(bot.pos, bot.forward(), &target).is_some() {
                    hits += 1;
                }
            }
        }
        hits
    }

    #[test]
    fn an_ace_outshoots_a_rookie() {
        let ace = duel(Skill::Ace, 40.0);
        let rookie = duel(Skill::Rookie, 40.0);
        assert!(ace > rookie, "ace {ace} vs rookie {rookie}");
        assert!(ace > 200, "ace should land many hits, got {ace}");
    }

    #[test]
    fn a_hard_turn_escapes_every_bot() {
        // The target turns at full rate; no bot can hold its firing cone.
        for skill in SKILLS {
            let mut bot = unprotected(Plane::new(1, Vec3::new(0.0, 0.0, 200.0), Quat::IDENTITY, 0.0));
            let mut target = unprotected(Plane::new(2, Vec3::ZERO, Quat::IDENTITY, 0.0));
            let dt = 1.0 / 60.0;
            let mut hits = 0;
            let mut cooldown = 0.0;
            for _ in 0..(20.0 * 60.0) as usize {
                let action = fly(&bot, skill, Some(&target), 10.0);
                bot.step(dt, &action.controls);
                target.step(dt, &Controls { yaw: 1.0, ..Default::default() });
                cooldown -= dt;
                if action.fire && cooldown <= 0.0 {
                    cooldown = 1.0 / tuning::GUN_RATE;
                    if crate::combat::ray_hit(bot.pos, bot.forward(), &target).is_some() {
                        hits += 1;
                    }
                }
            }
            assert!(
                hits < 40,
                "{skill:?} held the cone through a full turn: {hits} hits in 20 s"
            );
        }
    }

    #[test]
    fn bots_ignore_protected_planes() {
        let me = unprotected(Plane::new(1, Vec3::ZERO, Quat::IDENTITY, 0.0));
        let protected = Plane::new(2, Vec3::new(0.0, 0.0, -100.0), Quat::IDENTITY, 5.0);
        let planes = [me.clone(), protected];
        assert!(pick_target(&me, planes.iter(), 0.0).is_none());
    }

    #[test]
    fn a_rookie_has_no_missiles() {
        assert_eq!(Skill::Rookie.missile_range(), None);
        assert!(Skill::Ace.missile_range().unwrap() > 500.0);
    }
}

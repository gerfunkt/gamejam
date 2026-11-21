use crate::flight::{Part, Plane, PARTS};
use crate::tuning;
use glam::Vec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The target is protected after a spawn; nothing happened.
    Protected,
    Damaged,
    Broken,
    Killed,
}

/// The nearest part sphere a ray from `origin` along `dir` hits, if any.
pub fn ray_hit(origin: Vec3, dir: Vec3, plane: &Plane) -> Option<(Part, f32)> {
    let mut best: Option<(Part, f32)> = None;
    for part in PARTS {
        if plane.parts.health(part) == 0 {
            continue;
        }
        let center = plane.part_center(part);
        let radius = part.radius();
        let to = center - origin;
        let along = to.dot(dir);
        if along < 0.0 || along > tuning::GUN_RANGE {
            continue;
        }
        let closest = origin + dir * along;
        if (closest - center).length_squared() <= radius * radius {
            let dist = along - (radius * radius - (closest - center).length_squared()).sqrt();
            if best.map_or(true, |(_, d)| dist < d) {
                best = Some((part, dist.max(0.0)));
            }
        }
    }
    best
}

/// Apply damage to one part of a plane. A broken fuselage kills.
pub fn apply_damage(plane: &mut Plane, part: Part, amount: u8, now: f32) -> Outcome {
    if plane.protected(now) || !plane.alive {
        return Outcome::Protected;
    }
    let before = plane.parts.health(part);
    if before == 0 {
        return Outcome::Damaged;
    }
    plane.parts.damage(part, amount);
    if plane.parts.fuselage == 0 {
        plane.alive = false;
        plane.respawn_in = tuning::RESPAWN_S;
        return Outcome::Killed;
    }
    if plane.parts.health(part) == 0 {
        return Outcome::Broken;
    }
    Outcome::Damaged
}

/// Damage from a burst at `pos`: every part within BURST_RADIUS takes
/// BURST_CENTER at the centre falling to BURST_EDGE at the edge.
pub fn burst_damage(plane: &mut Plane, pos: Vec3, now: f32) -> Vec<(Part, Outcome)> {
    let mut out = Vec::new();
    if plane.protected(now) || !plane.alive {
        return out;
    }
    for part in PARTS {
        if plane.parts.health(part) == 0 {
            continue;
        }
        let dist = (plane.part_center(part) - pos).length();
        if dist > tuning::BURST_RADIUS {
            continue;
        }
        let t = dist / tuning::BURST_RADIUS;
        let amount = tuning::BURST_CENTER + (tuning::BURST_EDGE - tuning::BURST_CENTER) * t;
        let outcome = apply_damage(plane, part, amount.round() as u8, now);
        out.push((part, outcome));
        if outcome == Outcome::Killed {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;

    fn target_at(pos: Vec3) -> Plane {
        let mut p = Plane::new(2, pos, Quat::IDENTITY, 0.0);
        p.protect_until = 0.0;
        p
    }

    #[test]
    fn a_ray_through_a_part_hits_it() {
        // The plane faces away; the ray meets its tail first.
        let target = target_at(Vec3::new(0.0, 0.0, -100.0));
        let hit = ray_hit(Vec3::ZERO, Vec3::NEG_Z, &target);
        assert_eq!(hit.map(|(part, _)| part), Some(Part::Tail));
    }

    #[test]
    fn a_ray_picks_the_nearest_part() {
        // Aimed at the right wing tip, the ray meets the right wing.
        let target = target_at(Vec3::new(0.0, 0.0, -100.0));
        let dir = Vec3::new(6.0, 0.0, -100.0).normalize();
        let hit = ray_hit(Vec3::ZERO, dir, &target);
        assert_eq!(hit.map(|(part, _)| part), Some(Part::RightWing));
    }

    #[test]
    fn a_ray_beyond_range_misses() {
        let target = target_at(Vec3::new(0.0, 0.0, -tuning::GUN_RANGE - 10.0));
        assert_eq!(ray_hit(Vec3::ZERO, Vec3::NEG_Z, &target), None);
    }

    #[test]
    fn protection_blocks_all_damage() {
        let mut target = Plane::new(2, Vec3::ZERO, Quat::IDENTITY, 0.0);
        let outcome = apply_damage(&mut target, Part::Fuselage, 200, 1.0);
        assert_eq!(outcome, Outcome::Protected);
        assert_eq!(target.parts.fuselage, tuning::FUSELAGE_HEALTH);
    }

    #[test]
    fn enough_damage_breaks_a_wing() {
        let mut target = target_at(Vec3::ZERO);
        for _ in 0..11 {
            assert_eq!(
                apply_damage(&mut target, Part::LeftWing, tuning::GUN_DAMAGE, 10.0),
                Outcome::Damaged
            );
        }
        assert_eq!(
            apply_damage(&mut target, Part::LeftWing, tuning::GUN_DAMAGE, 10.0),
            Outcome::Broken
        );
        assert_eq!(target.parts.left_wing, 0);
        assert!(target.alive);
    }

    #[test]
    fn a_broken_fuselage_kills_and_respawn_is_scheduled() {
        let mut target = target_at(Vec3::ZERO);
        let outcome = apply_damage(&mut target, Part::Fuselage, 200, 10.0);
        assert_eq!(outcome, Outcome::Killed);
        assert!(!target.alive);
        assert_eq!(target.respawn_in, tuning::RESPAWN_S);
    }

    #[test]
    fn a_near_burst_breaks_a_wing() {
        let mut target = target_at(Vec3::ZERO);
        // A burst on the left wing tip: inside the radius, near the edge.
        let outcomes = burst_damage(&mut target, Vec3::new(-8.0, 0.0, 0.0), 10.0);
        assert!(outcomes.iter().any(|(part, _)| *part == Part::LeftWing));
        let mut direct = target_at(Vec3::ZERO);
        burst_damage(&mut direct, Vec3::new(-6.0, 0.0, 0.0), 10.0);
        assert_eq!(
            direct.parts.left_wing, 0,
            "a burst at the wing should break it"
        );
    }

    #[test]
    fn a_far_burst_does_nothing() {
        let mut target = target_at(Vec3::ZERO);
        let outcomes = burst_damage(&mut target, Vec3::new(0.0, 0.0, 50.0), 10.0);
        assert!(outcomes.is_empty());
        assert_eq!(target.parts, Plane::new(2, Vec3::ZERO, Quat::IDENTITY, 0.0).parts);
    }
}

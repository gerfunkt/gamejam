use crate::model::{Bindings, TouchState};
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use sim::flight::{steer_toward, Controls, Plane};

#[derive(Clone, Copy, Debug, Default)]
pub struct PlayerInput {
    pub controls: Controls,
    pub fire: bool,
    pub missile: bool,
}

fn tilt_axis(deg: f32) -> f32 {
    let dead = 2.0;
    let full = 28.0;
    if deg.abs() <= dead {
        0.0
    } else {
        ((deg.abs() - dead) / (full - dead)).clamp(0.0, 1.0) * deg.signum()
    }
}

pub fn gather(
    keys: &ButtonInput<KeyCode>,
    mouse: &ButtonInput<MouseButton>,
    touches: &Touches,
    wheel: &AccumulatedMouseScroll,
    window: Option<&Window>,
    bindings: &Bindings,
    touch: &mut TouchState,
    now: f32,
    plane: &Plane,
    dt: f32,
) -> PlayerInput {
    let mut controls = Controls { throttle: plane.throttle, ..Default::default() };
    let touch_pos = touches.first_pressed_position().or_else(|| {
        if touch.enabled && mouse.pressed(MouseButton::Left) {
            window.and_then(|w| w.cursor_position())
        } else {
            None
        }
    });
    let cursor_pos = if touch.enabled {
        touch_pos
    } else {
        window.and_then(|w| w.cursor_position())
    };
    if let Some((window, cursor)) = window.zip(cursor_pos) {
        let in_field = !touch.enabled
            || (cursor.x > window.width() * 0.22
                && cursor.x < window.width() * 0.78
                && cursor.y < window.height() * 0.80);
        if in_field {
            let center = Vec2::new(window.width() * 0.5, window.height() * 0.5);
            let scale = center.x.max(1.0);
            let offset = (cursor - center) / scale;
            let rot = plane.rot();
            let aim = plane.pos
                + plane.forward() * 600.0
                + (rot * sim::Vec3::X) * offset.x.clamp(-1.0, 1.0) * 450.0
                + (rot * sim::Vec3::Y) * -offset.y.clamp(-1.0, 1.0) * 300.0;
            let (pitch, yaw) = steer_toward(plane, aim);
            controls.pitch = pitch;
            controls.yaw = yaw;
        }
    } else if touch.enabled && touch.tilt_enabled {
        let delta = crate::hooks::current_tilt() - touch.tilt_zero;
        let tx = tilt_axis(delta.x);
        let ty = tilt_axis(delta.y);
        if tx != 0.0 || ty != 0.0 {
            let rot = plane.rot();
            let aim = plane.pos
                + plane.forward() * 600.0
                + (rot * sim::Vec3::X) * tx * 450.0
                + (rot * sim::Vec3::Y) * -ty * 300.0;
            let (pitch, yaw) = steer_toward(plane, aim);
            controls.pitch = pitch;
            controls.yaw = yaw;
        }
    }
    if keys.pressed(bindings.turn_left) {
        controls.yaw -= 0.8;
    }
    if keys.pressed(bindings.turn_right) {
        controls.yaw += 0.8;
    }
    if keys.pressed(bindings.nose_up) {
        controls.pitch += 0.8;
    }
    if keys.pressed(bindings.nose_down) {
        controls.pitch -= 0.8;
    }
    controls.pitch = controls.pitch.clamp(-1.0, 1.0);
    controls.yaw = controls.yaw.clamp(-1.0, 1.0);

    let touch_throttle = std::mem::take(&mut touch.throttle_delta);
    let throttle_change = wheel.delta.y * 0.04
        + touch_throttle
        + if keys.pressed(bindings.throttle_up) { dt * 0.5 } else { 0.0 }
        - if keys.pressed(bindings.throttle_down) { dt * 0.5 } else { 0.0 };
    controls.throttle = (plane.throttle + throttle_change).clamp(0.0, 1.0);
    controls.boost = keys.pressed(bindings.boost) || now < touch.boost_until;

    let touch_missile = std::mem::take(&mut touch.missile);
    let mouse_fire = !touch.enabled && mouse.pressed(MouseButton::Left);
    PlayerInput {
        controls,
        fire: mouse_fire || keys.pressed(bindings.guns) || now < touch.gun_until,
        missile: mouse.just_pressed(MouseButton::Right)
            || keys.just_pressed(bindings.missile)
            || touch_missile,
    }
}

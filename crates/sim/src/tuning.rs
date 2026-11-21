//! All gameplay numbers live here, so playtests change one file.

pub const MAX_SPEED: f32 = 120.0;
pub const MIN_SPEED: f32 = 40.0;
pub const BOOST_MULT: f32 = 1.6;
pub const ACCEL: f32 = 40.0;
pub const PITCH_RATE: f32 = 1.6;
pub const YAW_RATE: f32 = 1.2;
pub const ROLL_RATE: f32 = 2.5;
pub const MAX_PITCH: f32 = 1.4;
pub const MAX_BANK: f32 = 1.2;
pub const BANK_ROLL: f32 = 0.6;
pub const STEER_GAIN: f32 = 2.5;
pub const WING_ROLL: f32 = 1.2;
pub const WING_PULL: f32 = 0.25;
pub const DEAD_ENGINE_DRAG: f32 = 20.0;
pub const DEAD_ENGINE_SINK: f32 = 0.15;
pub const TAIL_AUTHORITY: f32 = 0.4;

pub const WING_HEALTH: u8 = 45;
pub const TAIL_HEALTH: u8 = 45;
pub const ENGINE_HEALTH: u8 = 60;
pub const FUSELAGE_HEALTH: u8 = 100;

pub const GUN_RANGE: f32 = 600.0;
pub const GUN_RATE: f32 = 10.0;
pub const GUN_DAMAGE: u8 = 4;
pub const HIT_SPHERE: f32 = 2.5;

pub const MISSILE_SPEED: f32 = 220.0;
pub const MISSILE_TURN: f32 = 1.0;
pub const MISSILE_CONE: f32 = 0.45;
pub const MISSILE_FUEL: f32 = 6.0;
pub const MISSILE_GRACE: f32 = 2.0;
pub const MISSILE_DRAG: f32 = 120.0;
pub const BURST_RADIUS: f32 = 9.0;
pub const BURST_CENTER: f32 = 70.0;
pub const BURST_EDGE: f32 = 20.0;
pub const LOCK_TIME: f32 = 1.2;
pub const LOCK_CONE: f32 = 0.15;
pub const LOCK_RANGE: f32 = 700.0;

pub const RESPAWN_S: f32 = 3.0;
pub const PROTECT_S: f32 = 5.0;

pub const STATE_HZ: f32 = 30.0;

//! Every number of the rules, in one place, so that a playtest can change the feel of the game here.

/// The length of one step of the rules, in seconds.
pub const STEP: f32 = 1.0 / 60.0;
/// The most steps that one frame runs. After a long pause, the rest of the time is dropped.
pub const MAX_STEPS_PER_FRAME: u32 = 5;

// The speed of a plane.

/// The speed in level flight with the throttle closed, in m/s.
pub const MIN_SPEED: f32 = 45.0;
/// The speed in level flight with the throttle open, in m/s.
pub const MAX_SPEED: f32 = 110.0;
/// The speed in level flight under boost, in m/s.
pub const BOOST_SPEED: f32 = 160.0;
/// The speed that a plane with a dead engine keeps in level flight, in m/s.
pub const GLIDE_SPEED: f32 = 30.0;
/// The speed that a vertical climb costs and a vertical dive gives, in m/s.
pub const CLIMB_COST: f32 = 40.0;
/// How fast the engine raises the speed, in m/s².
pub const THRUST: f32 = 28.0;
/// How fast a dive raises the speed of a plane with a dead engine, in m/s².
pub const DIVE_GAIN: f32 = 10.0;
/// How fast the air lowers the speed, in m/s².
pub const DRAG: f32 = 14.0;
/// How long a full tank of boost lasts, in seconds.
pub const BOOST_SECONDS: f32 = 6.0;
/// The seconds of boost that one second without boost gives back.
pub const BOOST_RECHARGE: f32 = 0.4;

// The turn of a plane.

/// The pitch rate at full agility, in rad/s. Pitch is the strong axis, so a hard turn needs a bank.
pub const PITCH_RATE: f32 = 1.4;
/// The yaw rate at full agility, in rad/s.
pub const YAW_RATE: f32 = 0.45;
/// The roll rate at full agility, in rad/s.
pub const ROLL_RATE: f32 = 3.2;
/// The time in which the turn rates follow the commands, in seconds. It gives the plane weight.
pub const RESPONSE_TIME: f32 = 0.12;
/// The agility at each speed, as points of a line: a slow plane turns best, and a fast one worst.
pub const AGILITY: [(f32, f32); 5] = [(30.0, 0.3), (45.0, 1.0), (75.0, 1.0), (110.0, 0.8), (160.0, 0.5)];
/// The angle between the nose and the aim at which the steering reaches full deflection, in radians.
pub const STEER_FULL_ANGLE: f32 = 0.35;
/// The roll error at which the roll reaches full deflection, in radians.
pub const ROLL_FULL_ANGLE: f32 = 0.5;
/// Below this angle between the nose and the aim, the plane levels its wings, in radians.
pub const BANK_START_ANGLE: f32 = 0.05;
/// Above this angle between the nose and the aim, the plane banks fully into the turn, in radians.
pub const BANK_FULL_ANGLE: f32 = 0.4;

// Lift.

/// Below this speed, the wings give no lift, in m/s.
pub const STALL_LOW: f32 = 25.0;
/// Above this speed, the wings give full lift, in m/s.
pub const STALL_HIGH: f32 = 40.0;
/// How fast a plane without lift sinks, in m/s.
pub const SINK_SPEED: f32 = 30.0;
/// How fast the nose of a plane without lift drops, in rad/s.
pub const NOSE_DROP_RATE: f32 = 0.7;
/// The height above which the air gets too thin to fly, in metres.
pub const CEILING: f32 = 1500.0;
/// The height over which the lift fades above the ceiling, in metres.
pub const CEILING_FADE: f32 = 200.0;

// Damage that changes the flight.

/// The lift that stays with one wing, as a part of the full lift.
pub const ONE_WING_LIFT: f32 = 0.55;
/// The lift that stays with no wing, as a part of the full lift.
pub const NO_WING_LIFT: f32 = 0.1;
/// The roll control that stays with one wing, as a part of the full control.
pub const ONE_WING_ROLL: f32 = 0.5;
/// The roll control that stays with no wing, as a part of the full control.
pub const NO_WING_ROLL: f32 = 0.2;
/// How fast a lost wing rolls the plane toward its side, in rad/s.
pub const LOST_WING_ROLL: f32 = 1.2;
/// How fast a lost wing turns the nose toward its side, in rad/s.
pub const LOST_WING_YAW: f32 = 0.2;
/// The pitch and yaw control that stays without the tail, as a part of the full control.
pub const NO_TAIL_CONTROL: f32 = 0.3;

// The world.

/// A plane below this height hits the ground, in metres.
pub const GROUND_CLEARANCE: f32 = 2.0;
/// The distance from the centre beyond which a plane turns back by itself, in metres.
pub const ARENA_RADIUS: f32 = 2500.0;

// The health of the parts.

/// The health of the fuselage. A plane without it is destroyed.
pub const FUSELAGE_HEALTH: f32 = 100.0;
/// The health of one wing.
pub const WING_HEALTH: f32 = 45.0;
/// The health of the tail.
pub const TAIL_HEALTH: f32 = 40.0;
/// The health of the engine.
pub const ENGINE_HEALTH: f32 = 50.0;

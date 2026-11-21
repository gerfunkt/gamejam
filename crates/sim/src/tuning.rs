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

/// How long a plane with a lost wing falls before it is destroyed, in seconds.
pub const WING_LOSS_SECONDS: f32 = 3.0;
/// How fast a plane with a lost wing spins toward the side of that wing, in rad/s.
pub const WRECK_SPIN: f32 = 4.5;
/// How fast the nose of a plane with a lost wing drops, in rad/s.
pub const WRECK_NOSE_DROP: f32 = 1.4;
/// The acceleration of a plane with a lost wing toward the ground, in m/s².
pub const WRECK_FALL: f32 = 25.0;
/// The pitch and yaw control that stays without the tail, as a part of the full control.
pub const NO_TAIL_CONTROL: f32 = 0.3;

// The world.

/// A plane touches the terrain when its centre comes this near to it, in metres.
pub const PLANE_RADIUS: f32 = 2.0;
/// The height of the highest hill, in metres.
pub const HILL_HEIGHT: f32 = 140.0;
/// The number of blocks that stand on the ground.
pub const FEATURES: u32 = 160;
/// The height of the tallest block, in metres.
pub const FEATURE_MAX_HEIGHT: f32 = 90.0;
/// How deep a block stands in the ground, in metres.
pub const FEATURE_SUNK: f32 = 12.0;
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

// The gun.

/// The time between two bullets of one gun, in seconds.
pub const FIRE_INTERVAL: f32 = 0.1;
/// The speed of a bullet ahead of the plane that fires it, in m/s.
pub const BULLET_SPEED: f32 = 550.0;
/// How long a bullet flies, in seconds.
pub const BULLET_LIFE: f32 = 1.3;
/// The health that one bullet takes.
pub const BULLET_DAMAGE: f32 = 9.0;
/// The widest angle between a bullet and the nose, in radians.
pub const BULLET_SPREAD: f32 = 0.004;
/// The distance of the muzzle ahead of the centre of the plane, in metres.
pub const MUZZLE: f32 = 4.5;

// Missiles.

/// The number of missiles on a plane.
pub const MISSILES: u8 = 2;
/// The time until an empty place in the rack holds a missile again, in seconds.
pub const MISSILE_RELOAD: f32 = 8.0;
/// The time between two launches of one plane, in seconds.
pub const MISSILE_INTERVAL: f32 = 1.0;
/// How long the seeker must hold one target before a launch, in seconds. The target sees the
/// lock in this time, and can break away.
pub const LOCK_SECONDS: f32 = 1.2;
/// The farthest target that the seeker of a plane sees, in metres.
pub const LOCK_RANGE: f32 = 900.0;
/// The widest angle between the nose and a target that the seeker of a plane sees, in radians.
pub const LOCK_CONE: f32 = 0.22;
/// The speed of a missile when it leaves its plane, in m/s.
pub const MISSILE_LAUNCH_SPEED: f32 = 120.0;
/// How fast the motor of a missile raises its speed, in m/s².
pub const MISSILE_THRUST: f32 = 120.0;
/// The speed of a missile, in m/s.
pub const MISSILE_SPEED: f32 = 220.0;
/// How long a missile flies, in seconds.
pub const MISSILE_FUEL: f32 = 6.0;
/// How fast a missile turns, in rad/s.
pub const MISSILE_TURN_RATE: f32 = 0.7;
/// The widest angle between the nose of a missile and its target that its seeker sees, in radians.
pub const SEEKER_CONE: f32 = 0.45;
/// The part of the seeker cone that a missile uses to fly ahead of its target.
pub const SEEKER_LEAD: f32 = 0.7;
/// A missile bursts when it passes this near to a plane, in metres.
pub const MISSILE_FUSE: f32 = 7.0;
/// The distance from a burst within which a part takes damage, in metres.
pub const BURST_RADIUS: f32 = 9.0;
/// The health that a burst takes from a part at its centre.
pub const BURST_DAMAGE: f32 = 70.0;
/// The health that a burst takes from a part at the edge of its reach.
pub const BURST_EDGE_DAMAGE: f32 = 20.0;

// Flares.

/// The number of flares on a plane.
pub const FLARES: u8 = 3;
/// The time until an empty place in the rack holds a flare again, in seconds.
pub const FLARE_RELOAD: f32 = 12.0;
/// The time between two flares of one plane, in seconds.
pub const FLARE_INTERVAL: f32 = 1.0;
/// How long a flare burns, in seconds.
pub const FLARE_SECONDS: f32 = 3.0;
/// A flare lures only a missile that is still farther than this from its target, in metres.
/// A flare that comes later does not save the plane.
pub const FLARE_MIN_DISTANCE: f32 = 120.0;
/// The speed of a new flare along the flight of its plane, as a part of the speed of the plane.
pub const FLARE_SPEED: f32 = 0.6;
/// The speed with which a plane throws a flare downward, in m/s.
pub const FLARE_EJECT: f32 = 25.0;
/// How fast a flare loses its speed, as a part for each second.
pub const FLARE_DRAG: f32 = 1.0;
/// How fast a flare falls faster, in m/s².
pub const FLARE_FALL: f32 = 9.8;

// Bots.

/// How far ahead on its ring a bot without a target aims, in radians.
pub const BOT_IDLE_LEAD: f32 = 0.5;
/// The radius of the ring on which a bot without a target circles, in metres.
pub const BOT_IDLE_RADIUS: f32 = 600.0;
/// The height at which a bot without a target circles, in metres.
pub const BOT_IDLE_HEIGHT: f32 = 500.0;
/// Inside this distance of its target, a bot that matches speed does so, in metres.
pub const BOT_CHASE_RANGE: f32 = 400.0;
/// The distance behind its target at which such a bot holds its speed, in metres.
pub const BOT_CHASE_DISTANCE: f32 = 180.0;
/// A bot leaves the fight to fly level when it is nearer than this to the highest terrain below
/// it, or to the ceiling, in metres.
pub const BOT_SAFE_HEIGHT: f32 = 60.0;

/// How much a bot prefers a plane ahead of its nose: each radian off the nose counts as this
/// part of the distance again.
pub const BOT_ANGLE_WEIGHT: f32 = 1.0;
/// Inside this distance, a bot does not turn after a target that is far off its nose, in metres.
pub const BOT_EXTEND_RANGE: f32 = 250.0;
/// The angle off the nose beyond which a bot does not turn after a near target, in radians.
pub const BOT_EXTEND_ANGLE: f32 = 1.3;
/// The time that a bot allows for the end of a dive, in seconds.
pub const BOT_PULL_UP_SECONDS: f32 = 2.5;

// A match.

/// The number of steps between two states that a machine sends for each of its planes.
pub const STEPS_PER_STATE: u64 = 2;
/// The time between the end of a plane and its respawn, in seconds.
pub const RESPAWN_SECONDS: f32 = 3.0;
/// How long a plane is protected from damage after its spawn, in seconds.
pub const SPAWN_PROTECTION: f32 = 5.0;
/// How long after its last hit an attacker gets the kill for a crash of a target that lost no
/// part, in seconds. For a target that lost a part, the time has no limit.
pub const KILL_CREDIT_SECONDS: f32 = 10.0;
/// The nearest distance of a spawn from the centre, in metres.
pub const SPAWN_MIN_RADIUS: f32 = 500.0;
/// The farthest distance of a spawn from the centre, in metres.
pub const SPAWN_MAX_RADIUS: f32 = 900.0;
/// The lowest height of a spawn, in metres. It is above all terrain.
pub const SPAWN_MIN_HEIGHT: f32 = 400.0;
/// The greatest height of a spawn, in metres.
pub const SPAWN_MAX_HEIGHT: f32 = 600.0;

// Remote planes. These times are in milliseconds, as the clocks of the machines are.

/// The time between two states of one plane.
pub const STATE_INTERVAL: f64 = STEPS_PER_STATE as f64 * STEP as f64 * 1000.0;
/// The shortest delay at which a remote plane shows.
pub const MIN_DELAY: f64 = 50.0;
/// The longest delay at which a remote plane shows.
pub const MAX_DELAY: f64 = 400.0;
/// A margin above the measured unevenness, so that a state is usually there before it is needed.
pub const DELAY_MARGIN: f64 = 20.0;
/// How many times the measured unevenness the delay covers.
pub const JITTER_COVER: f64 = 2.5;
/// How fast the measured unevenness follows new arrivals, as a part for each arrival.
pub const JITTER_RESPONSE: f64 = 0.1;
/// How fast the delay may change, as a part of the time that passes. A change of 5% is too slow to see.
pub const DELAY_PACE: f64 = 0.05;
/// A delay that is this much too long jumps down at once. That happens after a stall of a page,
/// and one jump forward is better than many seconds at a wrong delay.
pub const DELAY_SNAP: f64 = 250.0;
/// The travel times of this long give the lowest travel time. Thus a slower route, or a sender
/// that stalled, counts after a few seconds.
pub const TRAVEL_WINDOW: f64 = 4000.0;
/// The longest time for which a plane without a newer state coasts on its heading.
pub const MAX_COAST: f64 = 250.0;
/// A timeline drops the states that are older than this, behind the newest.
pub const STATE_HISTORY: f64 = 2000.0;

use bevy::prelude::*;
use sim::bots::Skill;

#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Name,
    Menu,
    Rooms,
    Lobby,
    Settings,
    Match,
    Paused,
    Results,
    Error,
}

impl Default for Screen {
    fn default() -> Self {
        Self::Name
    }
}

#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrevScreen(pub Screen);

impl Default for PrevScreen {
    fn default() -> Self {
        Self(Screen::Menu)
    }
}

#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Solo,
    Online,
}

impl Default for Mode {
    fn default() -> Self {
        Self::Solo
    }
}

#[derive(Resource, Clone, Debug)]
pub struct Player {
    pub name: String,
}

impl Default for Player {
    fn default() -> Self {
        Self { name: "Pilot".into() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingAction {
    Guns,
    Missile,
    ThrottleUp,
    ThrottleDown,
    TurnLeft,
    TurnRight,
    NoseUp,
    NoseDown,
    Boost,
}

pub const BINDING_ACTIONS: [BindingAction; 9] = [
    BindingAction::Guns,
    BindingAction::Missile,
    BindingAction::ThrottleUp,
    BindingAction::ThrottleDown,
    BindingAction::TurnLeft,
    BindingAction::TurnRight,
    BindingAction::NoseUp,
    BindingAction::NoseDown,
    BindingAction::Boost,
];

impl BindingAction {
    pub fn name(self) -> &'static str {
        match self {
            Self::Guns => "Guns",
            Self::Missile => "Missile",
            Self::ThrottleUp => "Throttle up",
            Self::ThrottleDown => "Throttle down",
            Self::TurnLeft => "Turn left",
            Self::TurnRight => "Turn right",
            Self::NoseUp => "Nose up",
            Self::NoseDown => "Nose down",
            Self::Boost => "Boost",
        }
    }
}

#[derive(Resource, Clone, Debug)]
pub struct Bindings {
    pub guns: KeyCode,
    pub missile: KeyCode,
    pub throttle_up: KeyCode,
    pub throttle_down: KeyCode,
    pub turn_left: KeyCode,
    pub turn_right: KeyCode,
    pub nose_up: KeyCode,
    pub nose_down: KeyCode,
    pub boost: KeyCode,
}

impl Default for Bindings {
    fn default() -> Self {
        Self {
            guns: KeyCode::Space,
            missile: KeyCode::KeyF,
            throttle_up: KeyCode::KeyW,
            throttle_down: KeyCode::KeyS,
            turn_left: KeyCode::KeyA,
            turn_right: KeyCode::KeyD,
            nose_up: KeyCode::ArrowDown,
            nose_down: KeyCode::ArrowUp,
            boost: KeyCode::ShiftLeft,
        }
    }
}

impl Bindings {
    pub fn get(&self, action: BindingAction) -> KeyCode {
        match action {
            BindingAction::Guns => self.guns,
            BindingAction::Missile => self.missile,
            BindingAction::ThrottleUp => self.throttle_up,
            BindingAction::ThrottleDown => self.throttle_down,
            BindingAction::TurnLeft => self.turn_left,
            BindingAction::TurnRight => self.turn_right,
            BindingAction::NoseUp => self.nose_up,
            BindingAction::NoseDown => self.nose_down,
            BindingAction::Boost => self.boost,
        }
    }

    pub fn set(&mut self, action: BindingAction, key: KeyCode) {
        match action {
            BindingAction::Guns => self.guns = key,
            BindingAction::Missile => self.missile = key,
            BindingAction::ThrottleUp => self.throttle_up = key,
            BindingAction::ThrottleDown => self.throttle_down = key,
            BindingAction::TurnLeft => self.turn_left = key,
            BindingAction::TurnRight => self.turn_right = key,
            BindingAction::NoseUp => self.nose_up = key,
            BindingAction::NoseDown => self.nose_down = key,
            BindingAction::Boost => self.boost = key,
        }
    }

    pub fn save_str(&self) -> String {
        BINDING_ACTIONS
            .iter()
            .map(|&a| format!("{}={:?}", a.name(), self.get(a)))
            .collect::<Vec<_>>()
            .join(";")
    }

    pub fn load_str(&mut self, raw: &str) {
        for pair in raw.split(';') {
            let Some((name, key_str)) = pair.split_once('=') else {
                continue;
            };
            let Some(&action) = BINDING_ACTIONS.iter().find(|a| a.name() == name) else {
                continue;
            };
            if let Some(key) = parse_keycode(key_str) {
                self.set(action, key);
            }
        }
    }
}

fn parse_keycode(s: &str) -> Option<KeyCode> {
    use KeyCode::*;
    let candidates = [
        Space, KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM,
        KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
        Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
        ArrowUp, ArrowDown, ArrowLeft, ArrowRight, ShiftLeft, ShiftRight, ControlLeft,
        ControlRight, AltLeft, AltRight, Enter, Tab, Backspace,
    ];
    candidates.into_iter().find(|k| format!("{k:?}") == s)
}

#[derive(Clone, Debug)]
pub enum UiAction {
    Show(Screen),
    ChooseSolo,
    ChooseOnline,
    RefreshRooms,
    JoinRoom(String),
    CreateRoom,
    SubmitName,
    LobbyBots(i8),
    LobbySkill(i8),
    StartMatch,
    EndMatch,
    Kick(u32),
    RenameRoom,
    Rebind(BindingAction),
    Resume,
    LeaveMatch,
    TouchGun,
    TouchMissile,
    TouchBoost,
    TouchThrottle(f32),
    TouchCentre,
    TouchPause,
    ToggleTilt,
}

#[derive(Resource, Clone, Debug)]
pub struct TouchState {
    pub enabled: bool,
    pub tilt_enabled: bool,
    pub gun_until: f32,
    pub boost_until: f32,
    pub missile: bool,
    pub throttle_delta: f32,
    pub tilt_zero: Vec2,
}

impl Default for TouchState {
    fn default() -> Self {
        Self {
            enabled: false,
            tilt_enabled: true,
            gun_until: 0.0,
            boost_until: 0.0,
            missile: false,
            throttle_delta: 0.0,
            tilt_zero: Vec2::ZERO,
        }
    }
}

#[derive(Resource, Default)]
pub struct UiActions(pub Vec<UiAction>);

#[derive(Resource, Default)]
pub struct UiRefresh(pub bool);

#[derive(Resource, Default)]
pub struct WaitingForKey(pub Option<BindingAction>);

#[derive(Resource, Default, Debug)]
pub struct Stats {
    pub frames: u64,
    pub elapsed: f32,
    pub hits_sent: u32,
    pub hits_applied: u32,
    pub kills: u32,
    pub deaths: u32,
    pub missiles: u32,
    pub errors: u32,
}

impl Stats {
    pub fn fps(&self) -> f32 {
        if self.elapsed > 0.0 {
            self.frames as f32 / self.elapsed
        } else {
            0.0
        }
    }

    pub fn report(&self) -> String {
        format!(
            "MATCH_STATS fps={:.0} frames={} elapsed={:.1} hits_sent={} hits_applied={} kills={} deaths={} missiles={} errors={}",
            self.fps(),
            self.frames,
            self.elapsed,
            self.hits_sent,
            self.hits_applied,
            self.kills,
            self.deaths,
            self.missiles,
            self.errors
        )
    }
}

#[derive(Clone, Debug)]
pub struct LobbyView {
    pub lobby: sim::lobby::Lobby,
    pub profiles: std::collections::HashMap<u32, String>,
    pub room_title: String,
    pub you: Option<u32>,
    pub rooms: Vec<sim::room::Listed>,
    pub error: String,
}

impl Default for LobbyView {
    fn default() -> Self {
        Self {
            lobby: sim::lobby::Lobby { bots: 4, skill: Skill::Pilot, ..Default::default() },
            profiles: std::collections::HashMap::new(),
            room_title: String::new(),
            you: None,
            rooms: Vec::new(),
            error: String::new(),
        }
    }
}

#[derive(Resource, Default)]
pub struct SharedView(pub LobbyView);

#[derive(Resource, Default)]
pub struct Results(pub Vec<String>);

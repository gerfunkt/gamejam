use crate::hooks::Hooks;
use crate::model::*;
use crate::net::{self, Net};
use crate::online::{score_vec, start_online, OnlineGame};
use crate::render::RenderState;
use crate::solo::{score_lines, SoloGame};
use bevy::prelude::*;
use bevy::text::EditableText;
use sim::lobby::Lobby;

pub fn process_ui_actions(
    time: Res<Time>,
    mut actions: ResMut<UiActions>,
    (mut screen, mut prev_screen, mut mode, mut player, mut touch, mut waiting): (
        ResMut<Screen>,
        ResMut<PrevScreen>,
        ResMut<Mode>,
        ResMut<Player>,
        ResMut<TouchState>,
        ResMut<WaitingForKey>,
    ),
    (mut shared, mut results, mut stats, mut render): (
        ResMut<SharedView>,
        ResMut<Results>,
        ResMut<Stats>,
        ResMut<RenderState>,
    ),
    mut net: NonSendMut<Net>,
    mut commands: Commands,
    hooks: Res<Hooks>,
    solo: Option<ResMut<SoloGame>>,
    online: Option<ResMut<OnlineGame>>,
    names: Query<&EditableText, With<crate::ui::NameField>>,
) {
    let now = solo
        .as_ref()
        .map(|s| s.world.time)
        .or_else(|| online.as_ref().map(|o| o.time))
        .unwrap_or_else(|| time.elapsed_secs());
    for action in actions.0.drain(..) {
        match action {
            UiAction::Show(next) => {
                if next == Screen::Settings {
                    prev_screen.0 = *screen;
                }
                *screen = next;
                if next == Screen::Rooms {
                    net.fetch_rooms();
                }
            }
            UiAction::ChooseSolo => {
                *mode = Mode::Solo;
                shared.0.lobby = Lobby {
                    members: vec![sim::lobby::Member { num: 1, name: player.name.clone() }],
                    bots: hooks.bots.unwrap_or(4),
                    skill: hooks.skill.unwrap_or(sim::bots::Skill::Pilot),
                    running: false,
                };
                *screen = Screen::Lobby;
            }
            UiAction::ChooseOnline => {
                *mode = Mode::Online;
                if !net.connected() {
                    net.connect(hooks.room.as_deref(), hooks.create.as_deref());
                }
                net.fetch_rooms();
                *screen = Screen::Rooms;
            }
            UiAction::RefreshRooms => net.fetch_rooms(),
            UiAction::JoinRoom(room) => {
                net.close();
                net.connect(Some(&room), None);
                *screen = Screen::Lobby;
            }
            UiAction::CreateRoom => {
                net.close();
                net.connect(None, Some("Dogfight"));
                *screen = Screen::Lobby;
            }
            UiAction::SubmitName => {
                if let Some(text) = names.iter().next() {
                    let name = text.value().to_string();
                    let name = name.trim().chars().take(24).collect::<String>();
                    if !name.is_empty() {
                        player.name = name;
                        crate::hooks::store_name(&player.name);
                    }
                }
                *screen = Screen::Menu;
            }
            UiAction::LobbyBots(delta) => {
                let bots = (shared.0.lobby.bots as i8 + delta).clamp(0, sim::lobby::MAX_BOTS as i8) as u8;
                shared.0.lobby.bots = bots;
                if *mode == Mode::Online {
                    net::send_game(&net, &sim::wire::Message::Lobby(shared.0.lobby.clone()));
                }
            }
            UiAction::LobbySkill(delta) => {
                let skills = sim::bots::SKILLS;
                let at = skills.iter().position(|s| *s == shared.0.lobby.skill).unwrap_or(1) as i8;
                shared.0.lobby.skill = skills[(at + delta).rem_euclid(skills.len() as i8) as usize];
                if *mode == Mode::Online {
                    net::send_game(&net, &sim::wire::Message::Lobby(shared.0.lobby.clone()));
                }
            }
            UiAction::StartMatch => {
                stats::reset(&mut stats);
                results.0.clear();
                render.clear = true;
                if *mode == Mode::Solo {
                    commands.insert_resource(SoloGame::new(
                        shared.0.lobby.bots,
                        shared.0.lobby.skill,
                        hooks.pilot_bot,
                        hooks.seconds,
                    ));
                    *screen = Screen::Match;
                } else if let Some(you) = shared.0.you {
                    shared.0.lobby.running = true;
                    net::send_game(&net, &sim::wire::Message::Lobby(shared.0.lobby.clone()));
                    commands.insert_resource(start_online(
                        you,
                        &shared.0.lobby,
                        hooks.pilot_bot,
                        hooks.seconds,
                    ));
                    *screen = Screen::Match;
                }
            }
            UiAction::EndMatch => {
                if *mode == Mode::Solo {
                    crate::hooks::log(&stats.report());
                    if let Some(solo) = solo.as_ref() {
                        results.0 = score_lines(&solo.scores);
                    } else {
                        results.0.push("Match ended".into());
                    }
                } else if let Some(game) = online.as_ref() {
                    let scores = score_vec(game);
                    net::send_game(&net, &sim::wire::Message::EndMatch { scores });
                    shared.0.lobby.running = false;
                    net::send_game(&net, &sim::wire::Message::Lobby(shared.0.lobby.clone()));
                    crate::hooks::log(&stats.report());
                }
                commands.remove_resource::<SoloGame>();
                commands.remove_resource::<OnlineGame>();
                render.clear = true;
                *screen = Screen::Results;
            }
            UiAction::Kick(num) => net::kick(&net, num),
            UiAction::RenameRoom => {
                net::room_title(&net, &format!("{}'s fight", player.name));
                net::room_info(&net, "dogfight lobby");
            }
            UiAction::Rebind(binding) => waiting.0 = Some(binding),
            UiAction::Resume => *screen = Screen::Match,
            UiAction::LeaveMatch => {
                net.close();
                commands.remove_resource::<SoloGame>();
                commands.remove_resource::<OnlineGame>();
                render.clear = true;
                shared.0.lobby.running = false;
                *screen = Screen::Menu;
            }
            UiAction::TouchGun => touch.gun_until = now + 0.6,
            UiAction::TouchMissile => touch.missile = true,
            UiAction::TouchBoost => touch.boost_until = now + 0.8,
            UiAction::TouchThrottle(delta) => touch.throttle_delta += delta,
            UiAction::TouchCentre => touch.tilt_zero = crate::hooks::current_tilt(),
            UiAction::TouchPause => *screen = Screen::Paused,
            UiAction::ToggleTilt => {
                touch.tilt_enabled = !touch.tilt_enabled;
            }
        }
    }
}

pub fn capture_rebind(
    keys: Res<ButtonInput<KeyCode>>,
    mut waiting: ResMut<WaitingForKey>,
    mut bindings: ResMut<Bindings>,
    mut refresh: ResMut<UiRefresh>,
) {
    let Some(action) = waiting.0 else {
        return;
    };
    if let Some(key) = keys.get_just_pressed().next().copied() {
        if key != KeyCode::Escape {
            bindings.set(action, key);
            crate::hooks::store_bindings(&bindings.save_str());
        }
        waiting.0 = None;
        refresh.0 = true;
    }
}

pub fn pause_key(
    keys: Res<ButtonInput<KeyCode>>,
    waiting: Res<WaitingForKey>,
    mode: Res<Mode>,
    hooks: Res<Hooks>,
    mut screen: ResMut<Screen>,
) {
    if waiting.0.is_some() {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        *screen = match *screen {
            Screen::Match => Screen::Paused,
            Screen::Paused => Screen::Match,
            screen => screen,
        };
    } else if *screen == Screen::Match
        && *mode == Mode::Solo
        && !hooks.pilot_bot
        && hooks.auto.is_none()
        && !crate::hooks::document_focused()
    {
        *screen = Screen::Paused;
    }
}

mod stats {
    use crate::model::Stats;

    pub fn reset(stats: &mut Stats) {
        *stats = Stats::default();
    }
}

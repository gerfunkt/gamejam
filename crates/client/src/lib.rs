use wasm_bindgen::prelude::*;

mod actions;
mod hooks;
mod input;
mod model;
mod net;
mod online;
mod render;
mod solo;
mod ui;

use actions::{capture_rebind, pause_key, process_ui_actions};
use hooks::Hooks;
use model::*;
use net::Net;
use render::{setup_scene, sync_render, update_transients, RenderState};
use solo::{step_solo, SoloGame};
use online::{net_events, step_online};
use ui::{action_observer, rebuild_ui, update_hud};

use bevy::prelude::*;

#[wasm_bindgen(start)]
pub fn run() {
    console_error_panic_hook::set_once();
    let hooks = Hooks::get();
    if !hooks::webgpu_available() {
        hooks::show_error("This game needs WebGPU. Use a current Chromium browser.");
        return;
    }

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "LAN Dogfight".into(),
            fit_canvas_to_parent: true,
            prevent_default_event_handling: true,
            ..default()
        }),
        ..default()
    }))
    .insert_resource(hooks)
    .init_resource::<Screen>()
    .init_resource::<PrevScreen>()
    .init_resource::<Mode>()
    .init_resource::<Player>()
    .init_resource::<Bindings>()
    .init_resource::<TouchState>()
    .init_resource::<UiActions>()
    .init_resource::<UiRefresh>()
    .init_resource::<WaitingForKey>()
    .init_resource::<SharedView>()
    .init_resource::<Results>()
    .init_resource::<Stats>()
    .init_resource::<RenderState>()
    .insert_non_send(Net::default())
    .add_observer(action_observer)
    .add_systems(Startup, (setup_scene, auto_start).chain())
    .add_systems(
        Update,
        (
            net_events,
            process_ui_actions,
            capture_rebind,
            pause_key,
            name_enter,
            step_solo,
            step_online,
            update_stats,
            sync_render,
            update_transients,
            rebuild_ui,
            update_hud,
        )
            .chain(),
    );
    app.run();
}

fn auto_start(
    hooks: Res<Hooks>,
    mut screen: ResMut<Screen>,
    mut mode: ResMut<Mode>,
    mut player: ResMut<Player>,
    mut bindings: ResMut<Bindings>,
    mut touch: ResMut<TouchState>,
    mut shared: ResMut<SharedView>,
    mut net: NonSendMut<Net>,
    mut commands: Commands,
) {
    touch.enabled = hooks.touch || hooks::is_touch_device();
    if touch.enabled {
        hooks::install_tilt_listener();
    }
    if let Some(name) = hooks.name.clone().or_else(hooks::stored_name) {
        player.name = name;
    }
    if let Some(raw) = hooks::stored_bindings() {
        bindings.load_str(&raw);
    }
    if let Some(bots) = hooks.bots {
        shared.0.lobby.bots = bots;
    }
    if let Some(skill) = hooks.skill {
        shared.0.lobby.skill = skill;
    }
    match hooks.auto.as_deref() {
        Some("solo") => {
            *mode = Mode::Solo;
            shared.0.lobby.bots = hooks.bots.unwrap_or(4);
            shared.0.lobby.skill = hooks.skill.unwrap_or(sim::bots::Skill::Pilot);
            commands.insert_resource(SoloGame::new(
                shared.0.lobby.bots,
                shared.0.lobby.skill,
                hooks.pilot_bot,
                hooks.seconds,
            ));
            *screen = Screen::Match;
        }
        Some("online") => {
            *mode = Mode::Online;
            net.connect(hooks.room.as_deref(), hooks.create.as_deref());
            *screen = Screen::Lobby;
        }
        _ => {
            *screen = if player.name != "Pilot" || hooks.name.is_some() {
                Screen::Menu
            } else {
                Screen::Name
            };
            if let Some(hook_screen) = hooks.screen.as_deref() {
                *screen = match hook_screen {
                    "settings" => Screen::Settings,
                    "rooms" => Screen::Rooms,
                    "lobby" => Screen::Lobby,
                    _ => *screen,
                };
                if *screen == Screen::Rooms {
                    net.fetch_rooms();
                }
            }
        }
    }
}

fn name_enter(
    keys: Res<ButtonInput<KeyCode>>,
    screen: Res<Screen>,
    mut actions: ResMut<UiActions>,
) {
    if *screen == Screen::Name && keys.just_pressed(KeyCode::Enter) {
        actions.0.push(UiAction::SubmitName);
    }
}

fn update_stats(
    time: Res<Time>,
    screen: Res<Screen>,
    hooks: Res<Hooks>,
    mut stats: ResMut<Stats>,
) {
    if *screen != Screen::Match {
        return;
    }
    stats.elapsed += time.delta_secs();
    stats.frames += 1;
    if hooks.stat && hooks.seconds.is_none() && stats.frames % 120 == 0 {
        hooks::log(&stats.report());
    }
}

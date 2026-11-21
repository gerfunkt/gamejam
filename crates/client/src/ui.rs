use crate::model::*;
use bevy::input_focus::AutoFocus;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::{EditableText, FontSize};
use bevy::ui_widgets::{Activate, ActivateOnPress, Button, TextInput};

#[derive(Component)]
pub(crate) struct UiRoot;

#[derive(Component)]
pub struct NameField;

#[derive(Component)]
pub(crate) struct HudText;

#[derive(Component, Clone)]
pub(crate) struct MenuAction(UiAction);

pub fn action_observer(
    activate: On<Activate>,
    actions: Query<&MenuAction>,
    mut queue: ResMut<UiActions>,
) {
    if let Ok(action) = actions.get(activate.entity) {
        queue.0.push(action.0.clone());
    }
}

pub fn rebuild_ui(
    mut commands: Commands,
    screen: Res<Screen>,
    prev_screen: Res<PrevScreen>,
    mut refresh: ResMut<UiRefresh>,
    shared: Res<SharedView>,
    player: Res<Player>,
    bindings: Res<Bindings>,
    touch: Res<TouchState>,
    waiting: Res<WaitingForKey>,
    results: Res<Results>,
    stats: Res<Stats>,
    roots: Query<Entity, With<UiRoot>>,
) {
    let changed = screen.is_changed()
        || refresh.0
        || shared.is_changed()
        || results.is_changed()
        || waiting.is_changed()
        || bindings.is_changed()
        || touch.is_changed();
    if !changed {
        return;
    }
    refresh.0 = false;
    for root in &roots {
        commands.entity(root).despawn();
    }
    let bg = if *screen == Screen::Match {
        Color::NONE
    } else {
        Color::srgba(0.015, 0.02, 0.035, 0.88)
    };
    commands
        .spawn((
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(12),
                padding: px(20).all(),
                ..default()
            },
            BackgroundColor(bg),
            UiRoot,
        ))
        .with_children(|parent| match *screen {
            Screen::Name => name_screen(parent, &player),
            Screen::Menu => menu_screen(parent),
            Screen::Rooms => rooms_screen(parent, &shared),
            Screen::Lobby => lobby_screen(parent, &shared),
            Screen::Settings => settings_screen(parent, &bindings, &touch, &waiting, prev_screen.0),
            Screen::Match => match_screen(parent, &stats, &touch),
            Screen::Paused => paused_screen(parent, &touch),
            Screen::Results => results_screen(parent, &results),
            Screen::Error => error_screen(parent, &shared),
        });
}

fn title(text: &str) -> impl Bundle {
    (
        Text::new(text.to_string()),
        TextFont { font_size: FontSize::Px(44.0), ..default() },
        TextColor(Color::srgb(0.92, 0.96, 1.0)),
    )
}

fn label(text: impl Into<String>) -> impl Bundle {
    (
        Text::new(text.into()),
        TextFont { font_size: FontSize::Px(19.0), ..default() },
        TextColor(Color::srgb(0.78, 0.84, 0.92)),
    )
}

fn small(text: impl Into<String>) -> impl Bundle {
    (
        Text::new(text.into()),
        TextFont { font_size: FontSize::Px(15.0), ..default() },
        TextColor(Color::srgb(0.62, 0.70, 0.80)),
    )
}

fn button(text: impl Into<String>, action: UiAction) -> impl Bundle {
    (
        Button,
        ActivateOnPress,
        Hovered::default(),
        MenuAction(action),
        Node {
            min_width: px(190),
            height: px(44),
            border: px(1).all(),
            padding: UiRect::horizontal(px(16)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border_radius: BorderRadius::all(px(5)),
            ..default()
        },
        BorderColor::all(Color::srgb(0.25, 0.45, 0.65)),
        BackgroundColor(Color::srgb(0.08, 0.15, 0.24)),
        children![(
            Text::new(text.into()),
            TextFont { font_size: FontSize::Px(18.0), ..default() },
            TextColor(Color::srgb(0.90, 0.95, 1.0)),
        )],
    )
}

fn row() -> Node {
    Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        column_gap: px(8),
        ..default()
    }
}

fn name_screen(parent: &mut ChildSpawnerCommands, player: &Player) {
    parent.spawn(title("LAN DOGFIGHT"));
    parent.spawn(label("Enter your pilot name"));
    parent.spawn((
        TextInput,
        EditableText::new(player.name.clone()),
        AutoFocus,
        NameField,
        Node {
            width: px(300),
            height: px(46),
            border: px(1).all(),
            padding: UiRect::horizontal(px(10)),
            align_items: AlignItems::Center,
            ..default()
        },
        BorderColor::all(Color::srgb(0.35, 0.55, 0.75)),
        BackgroundColor(Color::srgb(0.05, 0.08, 0.12)),
        TextFont { font_size: FontSize::Px(22.0), ..default() },
        TextColor(Color::srgb(0.95, 0.95, 0.95)),
    ));
    parent.spawn(button("Fly", UiAction::SubmitName));
    parent.spawn(small("Mouse steers. Space fires. F launches a locked missile."));
}

fn menu_screen(parent: &mut ChildSpawnerCommands) {
    parent.spawn(title("LAN DOGFIGHT"));
    parent.spawn(button("Solo", UiAction::ChooseSolo));
    parent.spawn(button("Online", UiAction::ChooseOnline));
    parent.spawn(button("Settings", UiAction::Show(Screen::Settings)));
}

fn rooms_screen(parent: &mut ChildSpawnerCommands, shared: &SharedView) {
    parent.spawn(title("Rooms"));
    for room in &shared.0.rooms {
        parent.spawn(button(
            format!("{} — {} players", room.title, room.members),
            UiAction::JoinRoom(room.id.clone()),
        ));
        if let Some(info) = &room.info {
            parent.spawn(small(info.clone()));
        }
    }
    parent.spawn(row()).with_children(|row| {
        row.spawn(button("Create room", UiAction::CreateRoom));
        row.spawn(button("Refresh", UiAction::RefreshRooms));
    });
    parent.spawn(button("Back", UiAction::Show(Screen::Menu)));
}

fn lobby_screen(parent: &mut ChildSpawnerCommands, shared: &SharedView) {
    let lobby = &shared.0.lobby;
    let host = lobby.host() == shared.0.you || shared.0.you.is_none();
    parent.spawn(title(if shared.0.room_title.is_empty() {
        "Lobby"
    } else {
        &shared.0.room_title
    }));
    for member in &lobby.members {
        parent.spawn(label(format!(
            "{}{}",
            member.name,
            if Some(member.num) == lobby.host() { " — host" } else { "" }
        )));
    }
    parent.spawn(label(format!("Bots: {}", lobby.bots)));
    if host {
        parent.spawn(row()).with_children(|row| {
            row.spawn(button("- bot", UiAction::LobbyBots(-1)));
            row.spawn(button("+ bot", UiAction::LobbyBots(1)));
        });
        parent.spawn(row()).with_children(|row| {
            row.spawn(button("< skill", UiAction::LobbySkill(-1)));
            row.spawn(label(lobby.skill.name()));
            row.spawn(button("skill >", UiAction::LobbySkill(1)));
        });
        parent.spawn(button("Start match", UiAction::StartMatch));
        parent.spawn(button("Rename room", UiAction::RenameRoom));
        for member in lobby.members.iter().filter(|m| Some(m.num) != lobby.host()) {
            parent.spawn(button(format!("Remove {}", member.name), UiAction::Kick(member.num)));
        }
    } else {
        parent.spawn(label(format!("Skill: {}", lobby.skill.name())));
        parent.spawn(small("The host chooses the bots and starts the match."));
    }
    parent.spawn(button("Back", UiAction::Show(Screen::Rooms)));
}

fn settings_screen(
    parent: &mut ChildSpawnerCommands,
    bindings: &Bindings,
    touch: &TouchState,
    waiting: &WaitingForKey,
    back: Screen,
) {
    parent.spawn(title("Settings"));
    if let Some(action) = waiting.0 {
        parent.spawn(label(format!("Press a key for {}", action.name())));
    }
    for action in BINDING_ACTIONS {
        parent.spawn(button(
            format!("{}: {:?}", action.name(), bindings.get(action)),
            UiAction::Rebind(action),
        ));
    }
    parent.spawn(button(
        format!("Tilt: {}", if touch.tilt_enabled { "On" } else { "Off" }),
        UiAction::ToggleTilt,
    ));
    parent.spawn(small("The left mouse button always fires, and the right button launches a missile."));
    parent.spawn(button("Back", UiAction::Show(back)));
}

fn match_screen(parent: &mut ChildSpawnerCommands, stats: &Stats, touch: &TouchState) {
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(16),
            left: px(16),
            ..default()
        },
        Text::new(format!("{:.0} fps", stats.fps())),
        TextFont { font_size: FontSize::Px(18.0), ..default() },
        TextColor(Color::srgb(0.85, 0.95, 1.0)),
        HudText,
    ));
    if touch.enabled {
        parent
            .spawn(Node {
                position_type: PositionType::Absolute,
                top: px(16),
                right: px(16),
                column_gap: px(8),
                ..default()
            })
            .with_children(|top| {
                top.spawn(button("Centre", UiAction::TouchCentre));
                top.spawn(button("II", UiAction::TouchPause));
            });
        parent
            .spawn(Node {
                position_type: PositionType::Absolute,
                bottom: px(16),
                left: px(16),
                column_gap: px(8),
                ..default()
            })
            .with_children(|left| {
                left.spawn(button("+", UiAction::TouchThrottle(0.2)));
                left.spawn(button("-", UiAction::TouchThrottle(-0.2)));
                left.spawn(button("BOOST", UiAction::TouchBoost));
            });
        parent
            .spawn(Node {
                position_type: PositionType::Absolute,
                bottom: px(16),
                right: px(16),
                column_gap: px(8),
                ..default()
            })
            .with_children(|right| {
                right.spawn(button("GUN", UiAction::TouchGun));
                right.spawn(button("MSL", UiAction::TouchMissile));
            });
    }
}

fn paused_screen(parent: &mut ChildSpawnerCommands, touch: &TouchState) {
    parent.spawn(title("Paused"));
    parent.spawn(button("Resume", UiAction::Resume));
    if touch.enabled {
        parent.spawn(button(
            format!("Tilt: {}", if touch.tilt_enabled { "On" } else { "Off" }),
            UiAction::ToggleTilt,
        ));
    }
    parent.spawn(button("Settings", UiAction::Show(Screen::Settings)));
    parent.spawn(button("End match", UiAction::EndMatch));
    parent.spawn(button("Leave", UiAction::LeaveMatch));
}

fn results_screen(parent: &mut ChildSpawnerCommands, results: &Results) {
    parent.spawn(title("Results"));
    if results.0.is_empty() {
        parent.spawn(label("The match is over."));
    }
    for line in &results.0 {
        parent.spawn(label(line.clone()));
    }
    parent.spawn(button("Lobby", UiAction::Show(Screen::Lobby)));
    parent.spawn(button("Menu", UiAction::LeaveMatch));
}

fn error_screen(parent: &mut ChildSpawnerCommands, shared: &SharedView) {
    parent.spawn(title("Connection problem"));
    parent.spawn(label(if shared.0.error.is_empty() {
        "The connection failed."
    } else {
        &shared.0.error
    }));
    parent.spawn(button("Menu", UiAction::LeaveMatch));
}

pub fn update_hud(
    mut texts: Query<&mut Text, With<HudText>>,
    stats: Res<Stats>,
    render: Res<crate::render::RenderState>,
) {
    for mut text in &mut texts {
        let plane = render.own.and_then(|id| render.planes.get(&id));
        let parts = plane.map(|p| p.parts).unwrap_or_default();
        let shield = if plane.is_some_and(|p| p.protected) { " [SAFE]" } else { "" };
        let lock = if render.lock_pct >= 100 {
            "LOCKED".to_string()
        } else {
            format!("lock {}%", render.lock_pct)
        };
        **text = format!(
            "{:.0} fps | hull {}{shield} L:{} R:{} T:{} E:{} | {lock} | {:.0} m/s | K:{} D:{}",
            stats.fps(),
            parts.fuselage,
            parts.left_wing,
            parts.right_wing,
            parts.tail,
            parts.engine,
            plane.map(|p| p.speed).unwrap_or(0.0),
            stats.kills,
            stats.deaths,
        );
    }
}

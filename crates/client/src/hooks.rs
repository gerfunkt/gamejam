use sim::bots::Skill;

#[derive(bevy::prelude::Resource, Clone, Debug, Default)]
pub struct Hooks {
    pub auto: Option<String>,
    pub bots: Option<u8>,
    pub skill: Option<Skill>,
    pub seconds: Option<f32>,
    pub stat: bool,
    pub screen: Option<String>,
    pub name: Option<String>,
    pub room: Option<String>,
    pub create: Option<String>,
    pub pilot_bot: bool,
    pub touch: bool,
    pub pixels: f32,
}

impl Hooks {
    pub fn get() -> Self {
        let mut hooks = Self { pixels: 1.5, ..Default::default() };
        let Some(window) = web_sys::window() else {
            return hooks;
        };
        let Ok(search) = window.location().search() else {
            return hooks;
        };
        for pair in search.trim_start_matches('?').split('&') {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let value = decode(value);
            match key {
                "auto" => hooks.auto = Some(value),
                "bots" => hooks.bots = value.parse().ok(),
                "skill" => {
                    hooks.skill = match value.as_str() {
                        "rookie" => Some(Skill::Rookie),
                        "pilot" => Some(Skill::Pilot),
                        "veteran" => Some(Skill::Veteran),
                        "ace" => Some(Skill::Ace),
                        _ => None,
                    };
                }
                "seconds" => hooks.seconds = value.parse().ok(),
                "stat" => hooks.stat = value == "1",
                "screen" => hooks.screen = Some(value),
                "name" => hooks.name = Some(value),
                "room" => hooks.room = Some(value),
                "create" => hooks.create = Some(value),
                "pilot" => hooks.pilot_bot = value == "bot",
                "touch" => hooks.touch = value == "1",
                "pixels" => hooks.pixels = value.parse().unwrap_or(1.5),
                _ => {}
            }
        }
        hooks
    }
}

fn decode(text: &str) -> String {
    js_sys::decode_uri_component(text).ok().and_then(|s| s.as_string()).unwrap_or_else(|| text.into())
}

pub fn now_seconds() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now() / 1000.0)
        .unwrap_or(0.0)
}

pub fn stored_name() -> Option<String> {
    let storage = web_sys::window()?.local_storage().ok()??;
    storage.get_item("gamejam.name").ok().flatten().filter(|name| !name.is_empty())
}

pub fn store_name(name: &str) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok()).flatten() {
        let _ = storage.set_item("gamejam.name", name);
    }
}

pub fn stored_bindings() -> Option<String> {
    let storage = web_sys::window()?.local_storage().ok()??;
    storage.get_item("gamejam.bindings").ok().flatten().filter(|s| !s.is_empty())
}

pub fn store_bindings(raw: &str) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok()).flatten() {
        let _ = storage.set_item("gamejam.bindings", raw);
    }
}

pub fn document_focused() -> bool {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return true;
    };
    let Ok(func) = js_sys::Reflect::get(doc.as_ref(), &"hasFocus".into()) else {
        return true;
    };
    let Ok(func) = wasm_bindgen::JsCast::dyn_into::<js_sys::Function>(func) else {
        return true;
    };
    func.call0(doc.as_ref()).ok().and_then(|v| v.as_bool()).unwrap_or(true)
}

pub fn is_touch_device() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let nav = window.navigator();
    js_sys::Reflect::get(nav.as_ref(), &"maxTouchPoints".into())
        .ok()
        .and_then(|v| v.as_f64())
        .is_some_and(|n| n > 0.0)
}

pub fn install_tilt_listener() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let handler = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::Event| {
        let beta = js_sys::Reflect::get(event.as_ref(), &"beta".into())
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32;
        let gamma = js_sys::Reflect::get(event.as_ref(), &"gamma".into())
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32;
        let _ = js_sys::Reflect::set(&window, &"__gamejam_tilt_x".into(), &beta.into());
        let _ = js_sys::Reflect::set(&window, &"__gamejam_tilt_y".into(), &gamma.into());
    }) as Box<dyn FnMut(web_sys::Event)>);
    if let Some(win) = web_sys::window() {
        let _ = win.add_event_listener_with_callback(
            "deviceorientation",
            wasm_bindgen::JsCast::unchecked_ref(handler.as_ref()),
        );
    }
    handler.forget();
}

pub fn current_tilt() -> bevy::math::Vec2 {
    let Some(window) = web_sys::window() else {
        return bevy::math::Vec2::ZERO;
    };
    let x = js_sys::Reflect::get(&window, &"__gamejam_tilt_x".into())
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as f32;
    let y = js_sys::Reflect::get(&window, &"__gamejam_tilt_y".into())
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as f32;
    bevy::math::Vec2::new(x, y)
}

pub fn webgpu_available() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let navigator = window.navigator();
    let gpu = js_sys::Reflect::get(navigator.as_ref(), &"gpu".into()).unwrap_or_default();
    !gpu.is_undefined() && !gpu.is_null()
}

pub fn show_error(text: &str) {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    if let Some(body) = document.body() {
        body.set_inner_html(&format!(
            "<main style='font:18px system-ui;color:#eee;background:#111;min-height:100vh;display:grid;place-items:center;margin:0'><p>{text}</p></main>"
        ));
    }
}

pub fn log(text: &str) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::log_1(&text.into());
    #[cfg(not(target_arch = "wasm32"))]
    println!("{text}");
}

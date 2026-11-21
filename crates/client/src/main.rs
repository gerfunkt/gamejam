//! The browser client of the plane shooter.
//!
//! The rules of the game are in the crate `sim`. This crate gathers the input of the player,
//! runs the session of this machine, talks to the message bus, and draws. The engine draws the
//! sky, and the page holds the menus and the display of a match.
//!
//! The address of the page takes hooks for tests: `pilot=bot` lets a bot fly the plane of the
//! player, `stat=1` writes the numbers of the match to the console, `backend=webgl2` loads the
//! client for browsers without WebGPU, `touch=1` shows the controls of a phone, `rest=1` stops
//! the picture behind the menus, as on a slow device, and `clouds=0` leaves the clouds out of the sky.

use bevy::prelude::*;

mod audio;
mod effects;
mod flow;
mod hud;
mod input;
mod net;
mod page;
mod propeller;
mod puff;
mod sky;
mod view;

fn main() {
	page::report_panics();
	let window = Window {
		title: "Dogfight".into(),
		canvas: Some("#game".into()),
		fit_canvas_to_parent: true,
		// The page handles the keys and the pointer itself, and it stops only what a match uses.
		prevent_default_event_handling: false,
		..default()
	};
	App::new()
		.add_plugins(
			// The page reports a panic itself, because the engine can panic before its own reporter runs.
			DefaultPlugins
				.set(WindowPlugin {
					primary_window: Some(window),
					..default()
				})
				// The server holds no extra file beside an asset, so the engine must not ask for one.
				.set(AssetPlugin {
					meta_check: bevy::asset::AssetMetaCheck::Never,
					..default()
				})
				.disable::<bevy::app::PanicHandlerPlugin>(),
		)
		.add_plugins((
			flow::plugin,
			view::plugin,
			propeller::plugin,
			sky::plugin,
			puff::plugin,
			effects::plugin,
			hud::plugin,
			audio::plugin,
		))
		.run();
}

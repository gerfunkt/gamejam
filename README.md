# Dogfight

A 3D plane shooter for a game jam. It runs in a browser on one local network, alone against bots or with other players. Each hit can break a part off a plane, and a plane with a broken part flies worse. The name is a working title.

Pilots fly over a valley with mountains, a canyon, and a crater, under a sky with a sun and clouds. Guns, missiles, and flares decide the fights. Power-ups in the sky give speed, full weapons, or a repair, and kills earn a newer plane. The game exists to answer one question in a playtest: is it fun to chase a plane and shoot it apart?

## Install

[mise](https://mise.jdx.dev) installs the tools. Run this in the repository:

```sh
mise install
```

It installs Rust with the WebAssembly target, the `wasm-bindgen` tool, Mr. Boxington, Node, and pnpm. The build needs no C compiler.

## Play

```sh
cargo xtask serve
```

The command builds the game, starts the server, and prints one address for each network of the machine, such as `https://192.168.1.20:5201`. Open an address in a current Chromium browser, and accept the warning about the certificate once. Other machines on the network open the same address.

- The command builds the client for WebGPU and the client for WebGL2. A phone or an older browser without WebGPU gets the second one, with a notice that the picture can be slower.
- `cargo xtask serve --dev --webgpu` builds only the first client, without the long optimisation. Use it for a short loop of change and test.
- `cargo xtask serve -- --port 5210` passes options to the server.
- The server prints "failed the TLS handshake" for each browser that has not yet accepted the certificate warning. That is normal.
- Ctrl+C stops the server. Run the command again after a change of the code.

On a desktop, the plane turns toward the mouse cursor. The left button fires the gun, the right button launches a missile, and the wheel sets the throttle. On a phone, held sideways, a stick or the tilt of the phone steers, and buttons do the rest. "How to Play" in the menu explains the controls, and "Settings" changes the keys.

## Publish

The game builds on this machine, and GitHub only deploys it:

1. `cargo xtask publish` builds both clients, and pushes them to the branch `pages-build`. That branch holds only the built game, and each publication replaces it.
2. On GitHub, the workflow "Publish the game" deploys that branch: Actions, "Publish the game", "Run workflow".

Before the first run, switch Pages on once: Settings, Pages, Source "GitHub Actions".

Pages serves files only, so the published game plays solo at once. For online play, one computer of the network runs `cargo xtask serve`. The published page opens with a large link to that server: `https://192.168.0.87:5201`, or the address of `cargo xtask publish --server <address>`. Players can also stay on the published page, accept the certificate of the server once, and enter its address under "Game Server" in the room list. The address `…/?server=192.168.0.87:5201` does the same in one step.

## Develop

| Command | What it does |
|---|---|
| `mise run test` | Runs the tests of the rules and of the server. They need no browser. |
| `mise run check` | Checks the format and the lints of all crates. |
| `cargo xtask build` | Converts the plane models, and builds both clients into `web/`. `--dev --webgpu` builds the WebGPU client alone and fast. |
| `cargo xtask models --preview` | Converts the plane models of `assets/planes/planes.toml` alone, and draws a picture of each one beside its files. |
| `cargo xtask terrain --preview` | Bakes the terrain model into the height grid of the rules and the picture of the client. The raw model is not in git: `assets/terrain/terrain.toml` says where to put it. |
| `node play.ts solo https://localhost:5201/` | Plays a solo match in headless Chromium, in `tests/browser/` after `pnpm install`, while the server runs. |

The parts of the repository:

| Part | Content |
|---|---|
| `crates/sim` | The rules: flight, damage, guns, missiles, bots, the messages, and the lobby. Every number of the rules is in `sim::tuning`. |
| `crates/client` | The browser client. The Bevy engine draws the sky, the land, and the planes, and the page holds the menus and the display. |
| `crates/server` | The server of the LAN host: HTTPS, the game files, and the message bus with its rooms. |
| `crates/xtask` | The build tasks. |
| `assets/` | The source files of the plane models and the manifest of the terrain, each with a record of where it comes from. |
| `web/` | The page that loads the client. |
| `tests/browser` | The tests that play the game in a browser. |

## Documents

`requirements.md`, `design.md`, and `feasibility.md` hold the requirements, the plan, and the measurements. They come from the study that led to this repository, and they do not describe every detail of this code yet. The issue tracker of the project is git-bug: `git bug pull origin` fetches the issues, and `git bug bug status:open` lists the open work. The work lands on the branch `main`.

## Licence

The GNU Affero General Public License, version 3. See [LICENSE](LICENSE).

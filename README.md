# Dogfight

A 3D plane shooter for a game jam. It runs in a browser on one local network, alone against bots or with other players. Each hit can break a part off a plane, and a plane with a broken part flies worse. The name is a working title.

The game is a prototype in grey boxes. It exists to answer one question in a playtest: is it fun to chase a plane and shoot it apart?

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

- `cargo xtask serve --release` builds the faster client, and also the client for browsers without WebGPU. Use it for a playtest.
- `cargo xtask serve -- --port 5210` passes options to the server.
- Ctrl+C stops the server. Run the command again after a change of the code.

The plane turns toward the mouse cursor. The left button fires the gun, the right button launches a missile, and the wheel sets the throttle. "Settings" in the menu lists the keys, and changes them.

## Develop

| Command | What it does |
|---|---|
| `mise run test` | Runs the tests of the rules and of the server. They need no browser. |
| `mise run check` | Checks the format and the lints of all crates. |
| `cargo xtask build` | Converts the plane models, and builds the WebGPU client into `web/`. With `--all`, it also builds the WebGL2 client. |
| `cargo xtask models --preview` | Converts the plane models of `assets/planes/planes.toml` alone, and draws a picture of each one beside its files. |
| `node play.ts solo https://localhost:5201/` | Plays a solo match in headless Chromium, in `tests/browser/` after `pnpm install`, while the server runs. |

The parts of the repository:

| Part | Content |
|---|---|
| `crates/sim` | The rules: flight, damage, guns, missiles, bots, the messages, and the lobby. Every number of the rules is in `sim::tuning`. |
| `crates/client` | The browser client. The Bevy engine draws the sky, and the page holds the menus. |
| `crates/server` | The server of the LAN host: HTTPS, the game files, and the message bus with its rooms. |
| `crates/xtask` | The build tasks. |
| `assets/` | The source files of the plane models, with a manifest that records where each one comes from. |
| `web/` | The page that loads the client. |
| `tests/browser` | The tests that play the game in a browser. |

## Documents

`requirements.md`, `design.md`, and `feasibility.md` hold the requirements, the plan, and the measurements. They come from the study that led to this repository, and they do not describe every detail of this code yet. The issue tracker of the project is git-bug: `git bug bug` lists the plan and the open points.

## Licence

The GNU Affero General Public License, version 3. See [LICENSE](LICENSE).

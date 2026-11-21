# LAN Dogfight

A 3D multiplayer plane shooter for browsers on a LAN, written in Rust with Bevy 0.20 and WebGPU. Each plane has five breakable parts whose damage changes how the plane flies, and one ordered WebSocket message bus on the LAN host keeps every machine in step without input delay on the player's own plane.

## Documents

- [`design.md`](design.md) — Concept, architecture, rooms protocol, controls, test hooks, and the step-by-step design plan.
- [`requirements.md`](requirements.md) — Product, control, lobby, network, engineering, and security requirements with their standing.
- [`feasibility.md`](feasibility.md) — Measurements and technical findings that support the design.

## Commands

All tools (`rust` 1.100.0 with `wasm32-unknown-unknown`, `wasm-bindgen-cli` 0.2.130, `binaryen`, `node`, `git-bug`, `mbx`) are managed by [`mise`](https://mise.jdx.dev/).

```sh
# Run the unit tests of the rules (crates/sim) and the server bus (crates/server)
mise run test

# Build the WebAssembly client into web/app.js and web/app_bg.wasm
mise run build
mise run build:release

# Build the client and serve HTTPS + WebSocket bus on port 5201
mise run serve
mise run serve:release
```

To run the headless Chromium browser verification suite after starting the server:

```sh
cd tests/browser
mise exec -- npm run check
mise exec -- npm run play
```

## Layout

| Path | Description |
|---|---|
| `crates/sim` | Engine-free game rules: flight kinematics, five breakable parts, guns, missiles with seeker cones, four bot skill levels, lobby state, room frames, snapshot interpolation timeline, and wire encoding. |
| `crates/client` | Bevy 0.20 WebGPU client compiled to WebAssembly: mouse-aim and keyboard flight, phone touch/tilt layout, menus, key rebinding, room browser, lobby, solo mode, and online multiplayer. |
| `crates/server` | Pure-Rust HTTPS static server and WebSocket room message bus on port 5201 (`hyper`, `rustls` with `rustls-rustcrypto`, `rcgen` with `p256`). |
| `crates/xtask` | Workspace build and serve automation (`cargo xtask build`, `cargo xtask serve`). |
| `web/` | HTML entry page and audio assets. |
| `tests/browser` | Playwright headless Chromium match and lobby verification scripts. |

# Feasibility

This report covers the technical, engineering, and game design limits of the outline in `game-notes.md`. It was last updated on 2026-11-21. The plan that follows from it is in `design.md`. Art direction is out of scope.

Each finding has one of three marks:

- **Measured**: a probe in `probes/` or the prototype produced the result on this machine.
- **Read**: a source states it. The address follows.
- **Judged**: reasoning from the other findings. Nobody tested it.

The test machine is a laptop with an AMD Radeon 760M integrated GPU and 12 CPU threads. The browser is headless Chromium on Vulkan, at 1280 by 720 pixels.

## The direction

The current idea is a 3D multiplayer shooter with planes, in browsers on a LAN.

| Subject | Choice | State |
|---|---|---|
| Engine | Bevy 0.20, Rust, WebAssembly, WebGPU | Measured in a browser |
| Network | A WebSocket message bus on the LAN host | Measured |
| Topology | A star for the transport. Each player decides its own movement and hits. The bus orders shared events. | Measured in the prototype, with three browsers on one machine |
| Serving | One Rust program: HTTPS, the game files, and the bus on port 5201 | Measured |
| Destruction | Five breakable parts on each plane | Built in the prototype |
| Build | Pure Rust. No C compiler, and no C code compiled. | Measured |
| Camera input | Ruled out by the owner on 2026-11-21: the game uses no camera | The isolation was measured before the ruling |

Everything else in the notes is in "Considered and set aside" at the end.

## Performance

The probe is `probes/bevy-web`. Each load runs for 5 s to warm, and then 6 s of measurement. "Capped" runs follow the 60 Hz display. "Uncapped" runs show the true cost of a frame. A slow frame takes over 20 ms.

**Measured, WebGPU, capped at 60 Hz:**

| Load | Count | Frames a second | Slow frames |
|---|---|---|---|
| Crowd: animated, skinned character on a physics capsule, with shadows | 400 | 60 | 1 of 361 |
| Crowd | 800 | 60 | 20 of 361 |
| Crowd | 1,600 | 15 | all |
| Avian debris: awake rigid boxes in one pile | 1,000 | 60 | 26 of 361 |
| Avian debris | 2,000 | 4 | all |
| Rapier debris: the same boxes | 1,000 | 60 | none |
| Rapier debris | 2,000 | 35 | all |
| Destruction: walls of convex chunks that break, with 400 loose chunks | 960 chunks | 60 | none |
| Destruction | 1,920 chunks | 55 | 5 of 332 |
| Relativity: sphere of 2,562 vertices with a warp for each vertex | 1,600 | 60 | none |
| Relativity | 6,400 | 34 | all |
| Compute field: cells stepped by a compute shader each frame | 4,096 by 4,096 | 60 | none |

**Measured, uncapped, mean frame time:**

| Load | WebGPU | WebGL2 |
|---|---|---|
| Empty scene | 3.7 ms | 2.8 ms |
| Crowd of 400 | 6.6 ms | 8.6 ms |
| Relativity, 1,600 spheres (4.1 million vertices) | 7.6 ms | 10.2 ms |
| Compute field, 2,048 by 2,048 | 5.2 ms | fails |
| Compute field, 4,096 by 4,096 | 9.4 ms | fails |

What the numbers mean for a plane shooter:

- Sixteen planes with eight parts each are 128 rigid objects. The engine holds several times that with any physics engine. Performance does not limit the design.
- The CPU runs on one thread in a browser. Physics is the first thing that breaks, at about 1,000 awake bodies.
- The GPU has room for a visible simulation: a compute shader steps 16.7 million cells in about 6 ms. Clouds, smoke, or a terrain effect can use this.
- WebGL2 runs the scenes about 30% slower and cannot run compute. One binary holds one backend.

Limits of this measurement:

- The scenes use a far camera and simple materials. A close camera and detailed materials add cost that the probe does not measure.
- Headless Chromium on one laptop is one data point. Repeat the sweep on the weakest demo laptop.

## The prototype

The prototype is the game of `design.md`, steps 1 and 2 in part, in `crates/`. It is grey boxes with the whole loop: menus, key bindings, mouse flight, a gun, missiles, breakable parts, bots, solo, and online play.

| Part | Lines | Job |
|---|---|---|
| `crates/sim` | 1,700 | The rules, with 45 tests |
| `crates/client` | 3,700 | Input for mouse, keys, touch, and tilt, the screens with the lobby, camera, drawing, and the network |
| `crates/server` | 500 | HTTPS, the game files, and the bus |

**Measured, in headless Chromium on this machine, through the Rust server:**

| Run | Frames a second | Result |
|---|---|---|
| Solo, a bot flies the plane of the player, 45 s, release build | 60 | 10 kills among 5 planes, 24 missiles launched, no error |
| Online, three pages, 45 s, release build | 60 on each page | 143 hits sent between pages and 142 applied (one still in flight at the end). All pages agree on 15 kills. |
| Solo, first 6 s | 60 | No hit on any plane while the spawn protection lasts |
| Flight by mouse, without the automatic mode | 60 | The plane turned right with the cursor right of the centre. Escape paused the solo match, and "Resume" continued it. |
| Solo, release build, uncapped (first prototype) | 290 | About 3.5 ms a frame |

- **Measured.** The netcode design holds: each hit reaches the owner of its target once, and every page shows the same kills, because all of them read one ordered stream.
- **Measured.** A press and a release inside one frame lost the click on a Bevy 0.20 button. A tap on a touchpad does this. The buttons now act on the press, as Bevy recommends for menus.
- **Measured.** The first prototype used Vite+, which refuses to import a file from its `public/` directory. The Rust server replaced Vite+, and the page now imports the client plainly.

### Mouse control

- **Found by hand.** The first prototype steered only under pointer lock, and in a real Chromium the lock never took effect. A solo match stayed paused, and an online plane got no input.
- The game now needs no pointer lock. The cursor is the aim: the plane turns toward the point under the cursor, through `sim::steer_toward`, which the bots use too. A cursor at the centre flies straight. This is the "mouse joystick" of many arcade flight games.
- **Measured.** Bevy tracks only the focus of the canvas, so the pause on lost focus uses `document.hasFocus()`.
- The left button fires the guns, the right button launches a missile, and the wheel sets the throttle. The keyboard stays a full alternative, and every key can change in the settings.

### Missiles

The aim of the design: a missile is strong, and the target still has a decision.

- **Measured, with a throwaway simulation of 18 cases.** With the first numbers (a turn rate of 1.5 rad/s, aim ahead of the target), every missile hit, whatever the target did. A slower turn rate alone did not help, because the missile aims at the place where the target will be.
- **Measured.** A seeker cone fixes it. A missile that loses sight of its target, because the target broke hard across its nose, flies straight on. With a speed of 220 m/s, 6 s of fuel, a turn rate of 1.0 rad/s, and a cone of 0.45 rad, the tests of `sim` show these rules:
  - A plane that flies straight is hit.
  - An early break (300 m) escapes a head-on missile, and a late break (100 m) does not.
  - A break across the nose of the missile escapes it.
  - A boosting plane outruns a missile fired from 500 m behind.
- **Measured.** At first, bots launched at once and shot the player down within about 4 s. A launch now needs a lock held for 1.2 s, so the target sees the lock box close in and can break away.
- A burst damages every part within 9 m, from 70 points at the centre to 20 at the edge. A wing has 45 points, so a near burst breaks it.
- **Measured.** After the start of a match, bots met the player head-on within seconds. A plane is now protected for 5 s after each match spawn: it takes no damage, and bots leave it alone.

### Lobby and bot skill

- **Measured.** A lobby opens before each match, in solo and online play. It sets the number of bots (0 to 8) and their skill. Online, it lists the players, and the player with the lowest number hosts: only the host changes the bots and starts the match, and a player who arrives late joins the running match. The rules of the lobby live in `sim::lobby`, with tests.
- **Measured.** In a test with three browsers, the host flew the two bots that it set, the other pages flew none, and all pages showed the same skill.
- Four skill levels: Rookie, Pilot, Veteran, and Ace. A higher level aims further ahead of its target, turns harder, fires in a narrower cone, and launches missiles from farther away. A Rookie carries no missiles.
- **Measured, with a throwaway duel.** Every bot first flew straight through a slow target and scored almost nothing. A Pilot or better now matches the speed of its target at close range. Against a slow plane in a gentle turn, an Ace then scores 429 gun hits in 40 s and a Rookie 169.
- **Measured.** A tighter turn escapes every bot, because a bot cannot hold its aim inside its firing cone. This is a way out for a player, as with missiles.

### Phone controls

- **Measured, in an emulated phone at 915 by 412.** The phone's tilt moves the aim point that the mouse cursor moves on a desktop, so the flight and the camera stay one path. A finger dragged on the field aims too, and it works while another finger holds GUN. The lobby, the settings, and the pause menu work by touch.
- **Measured.** Bevy 0.20 marks a button as pressed one step after the press. A press and a release within one frame then leave the button marked, and it ignores the next press: three instant taps on "+" counted once. Taps of 80 ms counted every time, but a quick tap on a slow phone fits in one frame. The client now clears a stuck mark when no finger and no mouse button is down, and four instant taps count four.
- **Measured.** On the web, Bevy's scale factor does not lower the pixels that the canvas draws. The client draws a smaller canvas and enlarges it with CSS, so a phone draws at most 1.5 device pixels for each CSS pixel.
- A real phone must still show:
  - the tilt directions on both landscape sides, because headless emulation reports a portrait screen
  - whether the tilt feels right: a dead zone of 2°, and a full turn at 28°
  - the permission prompt of iOS on the first tap, and full screen with the landscape lock on Android
  - the frame rate on a phone GPU, and the reach of the thumbs

Not verified, because a headless browser cannot do it:

- The feel of the mouse steering, the camera, and the missile evasion. Only a person can judge it.
- The wheel, and the right button without the context menu, in a real browser.
- A lost connection, and the screen that reports it.

## Compute times

Contest time is short, so these numbers decide the loop of edit, build, and test.

| Step | Time |
|---|---|
| `mise run test`: build and run the 35 tests of `sim`, from clean | 4 s |
| `cargo check` of the client after a small change | under 1 s |
| `mise run build`: dev build of the client after a small change | 11 to 20 s |
| Dev build of the client from clean | 165 to 245 s |
| `mise run build:release` from clean | 139 to 166 s |
| `mise run build:release` after a small change | 13 to 15 s |
| One browser test of a match | 30 to 45 s |

- The dev profile optimises the dependencies fully and the game code lightly, and it writes no debug information. The first build pays for the engine once, and Mr. Boxington keeps it for every checkout.
- Put new rules in `sim` and test them there. A test of a rule takes seconds, and a browser test takes most of a minute.
- **Measured.** The dev build is 142 MB, and about half of it is function names, which keep crash reports readable. The release build drops them and is 64 MB. Use the release build for playtests on other machines.
- **Measured.** The shared cache of Mr. Boxington serves every checkout and worktree. After the first build of the engine, a release build after a small change takes about as long as a dev build.

## Bevy in the browser

- **Read.** The current release is Bevy 0.20.1 (2026-10-28). It needs Rust 1.97.1 or later. <https://bevy.org/news/>
- **Measured.** Rust 1.100.0 builds Bevy 0.20.1 for `wasm32-unknown-unknown`. A clean release build takes about 160 s. A rebuild after a change in the game code takes 40 to 60 s.
- **Measured.** Both backends draw in Chromium on the Radeon 760M.
- **Read.** The `webgpu` feature overrides `webgl2`. <https://github.com/bevyengine/bevy/blob/main/docs/cargo_features.md>
- **Measured.** Bevy 0.20 writes shaders in WESL. A custom vertex stage imports modules such as `bevy_pbr::render::mesh_functions`. Examples for older versions do not compile unchanged, so read the source of the installed version.
- **Read.** A shader that fails to compile gives no clear error on the web (issue 25342). Check each shader with a screenshot. <https://github.com/bevyengine/bevy/issues/25342>
- **Read.** Audio on the web has an open defect that causes pauses (issue 24896, filed against 0.18). Check it on 0.20 before the audio work. <https://github.com/bevyengine/bevy/issues/24896>
- **Read.** The glTF loader does not support Draco, meshopt, or Basis compression. Export plain GLB. <https://docs.rs/bevy_gltf/latest/bevy_gltf/>
- The size of the package is not a limit on a LAN. The WebGPU build is about 70 MB before optimisation.

## Network

### What a browser allows

- A page cannot listen for connections, and it cannot send UDP. It can open a WebSocket or a WebTransport session to a server, or a WebRTC channel to another browser after a server passed the first messages.
- A server exists in every design, because something serves the page. WebGPU needs a secure page, so that server holds a certificate.
- The browser implements the encryption of each of these transports. Rust code that uses them through `web-sys` holds no crypto.

### The choice: a WebSocket message bus

- Every player opens one WebSocket to the bus, through the HTTPS server that serves the game. The bus sends each message to all players in one order. It knows nothing about the game.
- Each player simulates its own plane and sends its state about 30 times a second. The others interpolate it. The local player has no input delay, and the game needs no prediction and no correction.
- A shot is decided on the machine of the shooter, which sees the hit and sends one event.
- Shared events, such as a kill, a pick-up, or a broken plane part, are messages too. The bus gives them one order, so every machine applies them in the same order, and the first claim wins.
- **Measured.** `probes/bus` is this bus: about 60 lines of pure Rust on `tokio-tungstenite`, with no crypto and no C code. With 64 simulated players that each send 60 messages a second, the median delay is 0.15 ms and the worst is 2.8 ms. With 16 players, the median is 0.04 ms.
- **Measured.** From Chromium through the HTTPS server, the median round trip is 0.4 ms and the 95th percentile is 0.7 ms.
- **Measured.** `crates/server` serves the bus over TLS (rustls with `rustls-rustcrypto`) on the port of the game. `server load localhost:5201 <clients> 10` gives a median of 0.05 ms and a 99th percentile of 0.14 ms with 16 players, and 0.16 ms and 0.71 ms with 64 players.

The costs:

- Any player can cheat, because each machine decides its own hits. This does not matter at a jam.
- TCP delays all messages after a lost packet. On wired LAN this does not happen in practice. On Wi-Fi it shows as a short hitch. This is not measured, because the test machine cannot drop packets.
- An object that two players push at once has no single owner. Give each such object one owner.

### Rooms

- The bus holds rooms, so that several games run on one LAN at once: a player makes a room with `/ws?create=<title>`, and the others find it at `/rooms` and join it with `/ws?room=<id>`. A room is the old bus among its members, and `/ws` with no query joins the open room, so the current client works unchanged. `design.md` gives the protocol.
- The owner of a room, the member with the lowest number, controls it with text frames (`info:`, `title:`, `kick:`), and the server tells each member the members and the owner in a text frame. Thus the game logic stays in the client, and the server reads no game message.
- **Measured.** The rooms cost the bus nothing that the load test can see. `server load localhost:5241 16 10`, run three times in turn against the old and the new server on the same machine, gave medians of 0.06 to 0.09 ms with the old one and 0.05 to 0.07 ms with the new one, and a 99th percentile of 0.7 to 1.7 ms with both. The machine was busier than when it measured 0.05 ms above.
- **Measured.** Two Chromium pages in the open room play a match as before: each sees the other, and the hits that one page sends equal the hits that the other takes.
- 16 players fit in a room, so the load test with 64 players needs four rooms now. The load test uses the open room only.
- Ten tests drive the rooms over in-memory streams, with no TLS and no HTTP, in 0.01 s.

### Netcode that the bus makes cheap

These features come almost free from one ordered stream of messages. They are the "powerful netcode" to show the judges.

- **Replay.** The stream is a complete record of a match. A program that stores it can replay any moment, such as the last kill from the view of the victim.
- **Late join and spectators.** A new player or a spectator reads the stream from a stored snapshot.
- **Fair conflicts.** Two players who claim one pick-up get the same answer on every machine, because the order is the same.
- **Smooth remote planes.** Built: see "Remote planes over an uneven network" below.
- **Hit feedback without delay.** The shooter sees the hit at once, because the shooter decides it.
- **A bot is a player.** A bot connects to the bus as one more client. It can run in a page or as a native program.

**Judged.** All of these are known techniques. Of them, only the smooth remote planes are built.

### Remote planes over an uneven network

- **The fault.** Over the OpenVPN link of a test LAN (about 35 ms), remote planes moved back and forth. The client timed each state by the gap between the last two arrivals. A VPN over TCP delivers states in bunches, so that gap fell to one frame, and the client pushed the plane up to six steps past its newest state. The next state pulled it back. A second fault came out in tests: the states carried the step count of the sender, which stands still while a page stalls, so after a one-second stall the receivers kept the largest delay.
- **The technique.** Snapshot interpolation in `sim::Timeline`. Each state carries the real clock of the sender. The receiver takes the clock offset as the fastest travel time in the last 4 s, measures the jitter, and draws the plane `send interval + 2.5 × jitter + 20 ms` in the past (50 to 400 ms), between two real states. The delay changes at 5 % of the elapsed time, so the motion stays smooth, and it jumps only forward, so a plane never replays motion. A plane with no newer state coasts for 0.25 s at most.
- **Measured.** `tests/browser/jitter.ts` delays traffic in order, as a VPN over TCP does (35 ms plus 0 to 80 ms). Two Chromium pages played 40 s through it: in all 40 two-second reports, no remote plane stepped backward along its heading (`back_m=0.00`), and the delay settled at 145 to 265 ms. Without the proxy the delay is 73 to 85 ms. Seven tests in `sim` cover bunches, a stall of the sender, and a slower route.
- **The cost.** Remote planes show 0.1 to 0.25 s in the past on such a link. The own plane and the own hits have no delay, because each player decides them.

### WebTransport

WebTransport gives what WebSocket lacks: datagrams that may be lost, and streams that do not block each other.

- **Read.** Chromium supports it, and it accepts a self-signed certificate through `serverCertificateHashes`, if the certificate is valid for less than two weeks. No authority and no click is needed. <https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/WebTransport>
- The page side is pure Rust through `web-sys`. The server side is the problem.
- **Measured.** A WebTransport server speaks QUIC, and QUIC needs TLS in the server program. `wtransport` 0.7.2 does not compile without `ring` or `aws-lc-rs`, which both hold C code. Its code names the C providers directly. `quinn` 0.11 offers TLS only together with `ring` or `aws-lc-rs`.
- **Judged.** A pure-Rust server needs a QUIC stack that accepts another crypto provider, plus the HTTP/3 layer of WebTransport on top, plus a provider with QUIC support. An earlier probe showed that such a provider can be written in about 150 lines. The rest is a project of its own.
- It also needs a second port for UDP beside the HTTPS port, because the web server cannot forward it.
- **Verdict.** WebTransport is the better transport, and it breaks the pure-Rust rule today. Start with WebSocket.

If Wi-Fi hitches appear in playtests, try these in order:

1. Wire the host, and put the access point close.
2. Send player states over several WebSockets in turn. Each socket is its own TCP stream, so a lost packet delays one state out of several, and newer states pass it. Events stay on one socket. This is a judgement, and nobody measured it.
3. Move player states to WebRTC channels between browsers, with the bus as the signalling path.
4. Accept a C crypto library on the host for a WebTransport server. The page stays pure Rust.

Write the game against a small interface with two calls, "send a state" and "send an event". Then the transport can change without a change to the game.

### Serving on a LAN

- **Read.** WebGPU and the camera work only in a secure context. `http://localhost` is one. `http://192.168.x.y` is not. <https://developer.mozilla.org/en-US/docs/Web/Security/Secure_Contexts>
- Thus the host serves the game over HTTPS, and the bus at `/ws` on the same address, because a secure page can open only a secure WebSocket.

**Is Vite needed?** No. Vite serves files, builds JavaScript, and reloads pages during development. This game has no JavaScript to build, and its page is one file. The first prototype used Vite+ only to serve files over HTTPS and to forward `/ws`, and it put a Node backend beside the Rust bus.

- **Measured.** `crates/server` is one Rust program of about 500 lines that replaces Vite+ and the bus: HTTPS, the game files, and the bus, on port 5201. It builds with no C compiler. Its TLS is `rustls` with the pure-Rust provider `rustls-rustcrypto`, and it writes its certificate with `rcgen` and an ECDSA P-256 key from `p256`.
- **Measured.** The server makes a self-signed certificate for `localhost` and each address of the machine, and keeps it in `.server/`. A player accepts it once, and a restart keeps it.
- **Measured.** TLS costs the bus almost nothing: the median round trip is 0.05 ms with 16 clients and 0.16 ms with 64, against 0.04 ms and 0.15 ms over plain WebSocket.
- **Measured.** `cargo xtask build` replaces the Node build script. It builds the client and runs `wasm-bindgen`. `cargo xtask serve` builds the client and then runs the server.
- **Measured.** Chromium completes the TLS 1.3 handshake with the pure-Rust provider, and every browser test runs through the server.
- `rustls-rustcrypto` is a pre-release (0.1.0-pre.0). It is acceptable for a LAN demo. For a public server, use `--cert` and `--key` with a certificate from an authority, and review the provider again.
- The server speaks HTTP/1.1 only. That is enough for one page and one WebSocket for each player.
- Not tested: several real machines on one LAN.

**What remains outside Rust.**

- The page holds one line of script, because a page can start WebAssembly only from a script. The check for WebGPU and the error message are in Rust.
- **Measured.** A browser can offer `navigator.gpu` and still have no GPU adapter. Bevy then panics while its renderer starts, before its own panic plugin can help. The client installs its own panic hook first, so the page then says that the game needs WebGPU.
- The browser tests are TypeScript, about 40 lines, because no Rust tool drives a browser as well as Playwright. **Measured.** Node 26 runs a `.ts` file as it is, by removing the types, so the tests need no build step. Type checking adds two development dependencies, `typescript` 7 and `@types/node`. `pnpm check` runs it in about 0.2 s.
- The rest is Rust, about 4,400 lines.

## Destruction on planes

The scaled-back goal is a small number of breakable parts on each plane.

- **Judged.** A plane is one body with about five to eight parts, such as two wings, the tail, the engine, and the fuselage. Each part has its own hit shape and its own health.
- A hit on a part lowers its health. At zero, the part breaks off, and the flight model changes: a lost wing rolls the plane, a lost tail removes control, a dead engine removes thrust.
- The break is one event on the bus: plane, part, and a seed. Every machine applies it, so all players see the same plane.
- The loose part is decoration. It falls with gravity and spin, and it disappears after a few seconds. It needs no collision.
- **Judged.** This needs no physics engine. The flight model is own code in any case, and a hit test is a ray against a few simple shapes. `parry3d` supplies ray tests if hand-written ones become awkward. Leaving out Rapier and Avian removes a large dependency.
- **Measured.** The cost is far inside the budget. The probe holds 960 breakable chunks with 400 loose ones at 60 fps, and a match of 16 planes has about 128 parts.
- **Judged.** Damage that changes how the plane flies is a decision for the player: fight on with one wing, or run. Damage that only lowers a number is not.
- The larger destruction probe stays in `probes/bevy-web/src/destruct.rs`, with walls that break and lose support. No ready library fits Bevy 0.20 and pure Rust. `blast-stress-solver` wraps C++ code.

## Builds without a C compiler

- **Measured.** The workspace builds and passes its tests with `cc`, `gcc`, `g++`, `clang`, and `ar` replaced by programs that fail and log each call. The build started from an empty target directory, and it covered Bevy with Rapier and Avian for the browser.
- **Measured.** No C code was compiled. The only logged calls came from the build script of `blake3`, which asks what the compiler supports and continues without one.
- **Linking.** Rust calls `cc` to link host programs, such as build scripts and macros. `tools/ld` replaces it. It is a driver of ten lines that calls `rust-lld`, the linker inside the Rust toolchain, and adds the start files of glibc. `.cargo/config.toml` names it as the linker.
- The driver needs two things from the system: glibc with its start files, and `libgcc_s.so.1` from the `libgcc` package. Both are part of a base Arch system and of Debian and Ubuntu. The driver passes the file of that library in the place of `-lgcc_s`, because a system without a C compiler has no `libgcc_s.so` for the linker to find.
- The prebuilt tools still contain code that came from C or C++: Node, `wasm-opt`, `wasm-bindgen`, and Chromium. The project compiles none of it.
- To repeat the check, put `tools/no-cc` first on `PATH`, set `NO_CC_LOG` to a file, and build with an empty target directory.
- A limit: the driver holds the paths of one Linux layout. Another operating system needs its own driver.
- **Measured.** The server and the build task pass the same check, with no call to a compiler at all. That includes their TLS, which uses the pure-Rust provider.

## Camera input

Technology readiness on ordinary laptops with a webcam:

- **Read.** MediaPipe Face Landmarker (Apache-2.0) gives a head pose in a browser. <https://developers.google.com/edge/mediapipe/solutions/vision/face_landmarker>
- **Read.** Webcam gaze is coarse. WebEyeTrack (2025) reports 2.3 cm of error after a short calibration. WebGazer is under GPLv3, and its maintenance ended in February 2026. <https://arxiv.org/abs/2508.19544>, <https://github.com/brownhci/WebGazer>
- **Read.** Tobii hardware has Windows SDKs only. <https://developer.tobii.com/product-integration/>
- **Read.** In studies, selection by gaze alone had the worst error rate and speed, and head tracking for view control did about as well as a mouse. <https://www.frontiersin.org/articles/10.3389/frvir.2025.1576962/full>, <https://downloads.hci.informatik.uni-wuerzburg.de/Heydn2019aa.pdf>
- **Judged.** Head pose is the ready technology, and a plane game has the best precedent for it: head pose turns the view, as TrackIR does in flight sims. Gaze can choose among a few large targets, and it cannot aim.

Design rules (<https://developer.tobii.com/?p=10418>, <https://developer.tobii.com/?p=9624>):

- The view moves toward the head or the gaze, with a small dead zone and a curve that is gentle near the centre.
- Give a key that sets the centre again.
- Gaze may highlight a target. A button confirms. Gaze alone never fires.
- The game is complete with keyboard, mouse, and gamepad. The player can turn camera input off.

Security (note 16):

- **Measured.** The development machine has no camera device. The agent develops with synthetic numbers.
- **Read.** A browser grants the camera to the top-level page. A tracker inside an iframe of the game protects nothing, because the game page can then open the camera itself.
- **Measured.** `probes/browser/isolation.mjs` tests the reverse design with a fake camera. A small "shell" page is the top level and owns the camera. The game runs in a sandboxed iframe from another origin. The shell opens the camera. The browser refuses the camera to the game. The game cannot read the page or the video element of the shell. The game receives the numbers that the shell sends.
- **Ruling.** The browser enforces the boundary. The guarantee holds if a person owns the shell page and its origin, and reviews each change to it. No person was named, and the owner ruled camera input out on 2026-11-21: the game uses no camera.

## Mobile browsers (note 19)

Note 19 asks whether a phone with its gyroscope makes a better game. This section is research only. Nothing on a phone was tested.

- **Read.** Chrome for Android has WebGPU since version 121, on Android 12 and later with Qualcomm and ARM GPUs. Chrome 146 added a compatibility mode for weaker GPUs, which a page asks for with `featureLevel: "compatibility"`. <https://developer.chrome.com/blog/new-in-webgpu-121>, <https://developer.chrome.com/blog/new-in-webgpu-146>
- **Read.** Orientation events work only in a secure context. Safari on iOS asks the player for permission, through `DeviceOrientationEvent.requestPermission()`, which a tap must call. <https://developer.mozilla.org/en-US/docs/Web/API/DeviceOrientationEvent/requestPermission_static>, <https://developer.mozilla.org/docs/Web/API/Device_orientation_events>
- **Measured.** The server already serves HTTPS, so the secure context is in place.
- **Judged.** The gyroscope maps well to this game: tilt the phone to aim, and the plane steers toward the aim through `sim::steer_toward`, as it does for the mouse. Two thumb buttons fire guns and missiles, and the throttle can stay at a fixed value with a boost button.
- **Judged.** The costs: a touch layout on the canvas, a permission button for iOS, a "centre" button, and a performance check, because a phone GPU is weaker than the laptop GPU of the measurements. The scenes measured here use about 4 to 8 ms on the laptop, so a phone has some room.
- **Judged.** The gyroscope reads no camera and no microphone, so the security rule of note 16 does not apply to it.
- The design plan lists this as a stretch goal, after a playtest on laptops.

## Considered and set aside

**iroh.** It works in a browser in pure Rust: an earlier probe supplied its QUIC crypto from the RustCrypto crates, and measured a 0.3 ms median round trip through a local relay. It was set aside because a browser reaches other peers only through a relay over TCP. On a LAN it thus behaves like a WebSocket to a server, with a crypto provider and a relay program added. The probes are in the git history before the commit "remove the iroh probes and the relay".

**An authoritative game server.** It stops cheats, and it costs a second build of the game and the hardest netcode. Nothing at a LAN jam pays for that.

**WebRTC between browsers.** It has unreliable channels. Browsers hide local addresses behind names that need multicast, which some networks block, and it still needs a server for signalling. It stays as the third step of the list above.

**Netcode libraries.** `lightyear` 0.31 and `bevy_replicon` 0.45 support Bevy 0.20 and replicate game state from an authoritative server. They fit an authoritative server, and the bus design does not need them.

**Relativity (notes 7, 12, 15).** The rendering works: a vertex stage that applies contraction and light delay draws 1,600 objects at 60 fps, and `probes/relativity` holds tested kinematics. The research found no evidence that "aim where the target is, and not where you see it" is fun. It also makes network delay and light delay look the same. It can return as a mode of the plane game, after the basic game is fun.

**Soulslike and battle royale (notes 10, 11, 17).** Performance allows 400 animated fighters. The cost is animation content, and bots for a demo with few players. A battle royale rule set, such as a closing zone and last plane flying, fits the plane shooter without that cost.

**Soft bodies.** Rapier 0.36 simulates ropes, cloth, and jelly, and it tears and cuts them (<https://dimforge.com/blog/2026/09/25/advanced-soft-bodies-for-games-in-the-rapier-physics-engine/>). `bevy_rapier3d` 0.37 supports Bevy 0.20 and builds for the browser. Measured: 25 jelly cubes of 125 particles run at 60 fps, and 50 run at 12 fps. The probe mode `soft` holds the test.

**Google ML Kit (note 6).** It supports Android and iOS only. <https://developers.google.com/ml-kit/guides>

**Inference backend (note 5).** A hosted model answers after one to three seconds, so it fits a taunt or a line on the death screen, and not combat. A page cannot hold an API key, so a small proxy on the LAN host adds it. The game must work with the model off.

**Asset services for Kimi Code (note 14).** Kimi Code 2.6.3 reads MCP servers from `.kimi-code/mcp.json` and skills from `~/.agents/skills/`, where the `game-assets` skill already is. Apify and ElevenLabs have hosted MCP servers. Tripo needs its REST interface, because its MCP server needs Blender. Planes are rigid models, which Tripo makes without rigging. No key is set, and nothing was called. Free plans limit commercial use.

## Development environment

- **Measured.** mise installs Rust 1.100.0 with the WebAssembly target, Node, `wasm-bindgen` 0.2.130, `wasm-opt`, and Mr. Boxington. `mbx build` wraps Cargo.
- **Measured.** The `wasm-bindgen` tool must have the same version as the crate in `Cargo.lock`. `mise.toml` pins it for that reason.
- **Measured.** System packages in use: `mesa` and `vulkan-radeon` for the GPU, and `gtk3`, `nss`, and `alsa-lib` for Chromium. `gcc` and `clang` are not in use.
- Chromium in a podman container needs none of the last three on the host. It needs an image of 1 GB or more and a GPU passed into the container, so the host browser is the cheaper path.

## Open questions

1. Paid or free plans for Tripo and ElevenLabs?

Decided by the owner on 2026-11-21: WebGPU only, no WebGL2 build; no camera input, so no shell page is needed; and the demo is hosted locally, which makes the wired-against-wireless question and the Wi-Fi steps moot.

Not tested yet: the feel of the controls by a person, packet loss on Wi-Fi, several real machines on one LAN, and the weakest demo laptop.

The next test is a playtest of the prototype by a person. It answers whether the flight and the shooting are fun. `design.md` says how to run it.

## How to run the probes

The game itself runs with `cargo xtask serve --release`. `design.md` gives the steps and the controls.

Run these in `probes/browser/` after `pnpm install`:

- `node build.mjs bevy-web bevy-webgpu webgpu` builds the WebGPU probe. Leave the last word out and name it `bevy-webgl2` for WebGL2.
- `node run.mjs "bevy.html?build=webgpu&mode=crowd&n=400" 60 shot.png` runs one load. The modes are `crowd`, `debris`, `rapier`, `destruct`, `relativity`, `field`, and `soft`.
- Set `CHROMIUM_ARGS` with `--disable-frame-rate-limit --disable-gpu-vsync` for an uncapped run.
- `node isolation.mjs` runs the camera isolation test.

Run these in `probes/`:

- `mbx test -p relativity`
- `cargo xtask serve --release`, and then `mbx run --release -p server -- load localhost:5201 16 10` in another terminal, for the load test of the bus.

For the LAN set-up, start the bus, run `pnpm install` and `pnpm exec vp dev` in `probes/lan/`, and then run `node lan.mjs https://<LAN address>:5173/bus.html` in `probes/browser/`.

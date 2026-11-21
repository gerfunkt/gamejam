# Requirements

This document collects what the project must do and must not do. It does not say how: `design.md` holds the design, and `feasibility.md` holds the evidence. It was last updated on 2026-11-21.

Each requirement names its source:

- **Notes**: a point of `game-notes.md`, with its number. That file belongs to the study that came before this repository, and the repository does not hold it.
- **Owner**: a decision of the project owner during the work.
- **Proposed**: a proposal from the research, which the owner accepted or did not reject. The owner can overrule each one.

Each requirement has a standing:

- **Proven**: a measurement, a test of the rules, or an automatic match in a browser showed it.
- **Proven in part**: the note says which part is missing.
- **Not proven**: built or planned, and nothing has shown it yet.
- **Disqualified**: tried, and ruled out. The note says what replaced it.

## The product

| Id | Requirement | Source | Standing |
|---|---|---|---|
| P1 | The game is a 3D multiplayer shooter with planes, played in browsers on one LAN. | Owner | Proven |
| P2 | The view is third person. The camera stays steady in every attitude of the plane, a dive straight down included. | Owner | Proven in part: the camera keeps its side near the vertical by design. No person has confirmed it since the owner reported the fault. |
| P3 | The game has play against bots and play against people. | Owner | Proven |
| P4 | Each plane has five parts that damage can break: the fuselage, the two wings, the tail, and the engine. A lost part changes the flight. A plane that loses a wing or its fuselage crashes within seconds. | Owner, note 18 | Proven: a plane without a wing is destroyed within 3 s |
| P5 | The planes carry missiles as well as guns. | Owner | Proven |
| P6 | The game impresses judges through its mechanics: strong netcode, visibly high computation, and behaviour that is simple to describe. | Notes 3 and 4, owner | Proven in part: smooth motion and instant controls are proven. Replay, spectators, and a GPU simulation are not built. |
| P7 | The game is fun before it grows. A playtest proves the chase and the shot before content is added. | Proposed | Not proven: no playtest with people yet |
| P8 | Bots keep the sky busy when few people play. | Proposed | Proven |
| P9 | A player can see a missile coming and escape it, and a new plane cannot be killed at once. | Proposed | Proven in part: bots escape and survive in automatic matches. No person has judged it. |
| P10 | A plane carries flares. A flare draws a missile away from the plane. | Owner | Proven in the rules, and a browser test releases one |
| P11 | Power-ups float in the air as icons. A plane that flies through one gets its help: more speed for a time, a refill of the weapons, or a repair. | Owner | Proven in the rules. No person has judged them in play. |
| P12 | A pilot flies a better plane model as its kills rise. | Owner | Not proven: built with three models, at 0, 3, and 6 kills. No test shows the change. |
| P13 | The ground is the terrain map of the owner, with its mountains, its canyon, and its crater. A plane that touches the terrain is destroyed, and a shot ends there. | Owner | Proven in the rules, and a browser match flies over it |

## Controls

| Id | Requirement | Source | Standing |
|---|---|---|---|
| C1 | The controls are simple: the mouse and a few keys on a desktop, and touch on a phone. | Owner | Proven |
| C2 | On a desktop, only the mouse steers. No key steers the plane. | Owner | Proven. The keys that turned and pitched the plane were removed on 2026-11-21. |
| C3 | The player can change each key in the settings. | Owner | Proven |
| C4 | Steering works in every current browser of the players. | Owner's test | Proven. Steering that captures the mouse pointer was disqualified, because it failed in the owner's browser. |
| C5 | A phone has its own controls, and it is held sideways. A stick or the gyroscope steers, and a button exists for every other control, so a phone needs no keyboard. The layout follows the common layouts of phone games. | Note 19, owner | Proven in part: a browser test with an emulated phone flies with the stick, the buttons, and the tilt. The owner reported that the buttons and the gyroscope failed on a real phone, and no real phone has run the new layout. |
| C6 | The game is complete with the mouse and the keys on a desktop, and with touch on a phone. Each other input is optional. | Proposed | Proven on a desktop, and with an emulated phone |

## The screens

| Id | Requirement | Source | Standing |
|---|---|---|---|
| D1 | The game can fill the screen, on a desktop and on a phone where the browser allows it. The entry screen offers full screen where the player sees it at once. | Owner | Proven in part: a browser test enters and leaves full screen from the pause menu. The offer on the entry screen is not built, and no real phone has shown it. |
| D2 | The display in flight is full: the health of each part, the speed, the height, the throttle, the boost, the weapons, the heading, the target, a radar, markers on other planes, alerts, and the scores. | Owner | Proven in part: the screenshots of the tests show it. No person has judged it. |
| D3 | The menu has a "How to Play" screen with a short text on the aim and the controls. | Owner | Not proven: built, and no test opens it |
| D4 | The entry screen gives a short rundown of the features of the game. | Owner | Not proven: not built |
| D5 | The page shows the progress of the load. When the game cannot start, the page says why and what to do. No device stays on the loading screen without a message. | Owner's report | Proven in part: a browser test loads the WebGL2 client and plays. The owner has not confirmed it on the devices that were stuck. |

## The picture and the sound

| Id | Requirement | Source | Standing |
|---|---|---|---|
| A1 | The planes use the models of the asset branch. The build cuts each model into the five parts of P4, so that a broken part disappears. | Owner | Proven: a test of the build, and the screenshots |
| A2 | A propeller that turns shows as a blur, and its blades do not show. | Owner | Not proven: not built |
| A3 | A missile has a very simple model. | Owner | Proven in part: built. No person has judged it. |
| A4 | The environment is not trivial: the sky has detail, a sun, and clouds with volume. | Owner | Not proven: in work |
| A5 | Explosions, trails, flares, and smoke look like clouds with volume. | Owner | Not proven: in work |
| A6 | The game plays the sounds of the audio branch. The cockpit voice warns on the events of the match, and the other sounds follow the actions of the plane. The over-G alert is not used. | Owner | Proven in part: a browser test checks that the events start their sounds. An agent cannot hear, so a person must judge the mix. |
| A7 | Assets that arrive on other branches come into the game: planes, audio, and terrain. Each has a record of its source and its licence. | Owner, proposed | Proven in part: the planes, the audio, and the terrain are in. The licence of the models and of the terrain is not recorded. |

## The lobby

| Id | Requirement | Source | Standing |
|---|---|---|---|
| L1 | Solo play and online play each have a proper lobby. | Owner | Proven |
| L2 | The lobby sets the number of bots and their skill. | Owner | Proven |
| L3 | A player names itself. | Owner | Proven, with letters that have accents and with other scripts |
| L4 | A player starts its own lobby. | Owner | Proven |
| L5 | Other players find that lobby. | Owner | Proven |
| L6 | The host manages its lobby: the bots, the name, who stays, and when a match starts and ends. | Owner | Proven |
| L7 | When the host leaves, the lobby continues with a new host. | Proposed | Proven, during a match too |

## The network

| Id | Requirement | Source | Standing |
|---|---|---|---|
| N1 | The players connect through one WebSocket message bus on the LAN host. | Owner | Proven. iroh was disqualified: in a browser it needs a relay, so it adds parts and gains nothing on a LAN. WebTransport was considered and is not needed. |
| N2 | The LAN host serves everything. The game needs no internet. | Owner | Proven |
| N3 | The game can be reached on every network of the host, a VPN included. | Owner's test | Proven |
| N4 | The planes of other players move on a smooth path over an uneven link, such as a VPN at 35 ms. | Owner's test | Proven in part: no step backward on a simulated link, in the tests of the rules. The owner has not confirmed it on the real one. |
| N5 | The own plane and the own hits have no network delay. | Proposed | Proven |
| N6 | Every machine agrees on the shared events of a match. | Proposed | Proven |
| N7 | The server stays simple and knows nothing about the game. | Proposed | Proven. A server that decides the whole game was disqualified as too costly for a jam. |
| N8 | One lobby holds up to 16 players, and several lobbies run at once. | Proposed | Proven in part: under generated load, and not with 16 real browsers |
| N9 | The test and serve ports are 5201 to 5299. | Owner | Proven |

## Engineering

| Id | Requirement | Source | Standing |
|---|---|---|---|
| E1 | The game is Bevy in Rust, compiled to WebAssembly. | Note 1 | Proven |
| E2 | The build is pure Rust. It compiles no C code, and it uses no GCC, no clang, and no zig, for compiling or for linking. | Owner | Proven in part: the host links with the linker of the Rust toolchain, and stand-ins that fail take the place of the C compilers. The check that records every call to a compiler is not yet in this repository. |
| E3 | Very little code is JavaScript. Where the project needs it, the code is TypeScript. | Owner | Proven |
| E4 | The server is in Rust. | Owner | Proven. A JavaScript dev server was tried and disqualified. |
| E5 | mise manages the tools where that makes sense, Rust 1.100 included, and Mr. Boxington caches builds. No tool comes through a deprecated installer. The tool file of the project lists no tool that the directory above it already provides. | Owner | Proven |
| E6 | The speed of the game does not depend on the frame rate. | Owner's question | Proven |
| E7 | Claims about performance come from real runs. | Owner | Proven: 60 frames a second on a laptop with an integrated GPU |
| E8 | The loop of change, build, and test stays short, because contest time is short. | Owner | Proven: seconds for the rules, and under half a minute for the WebGPU client without the long optimisation |
| E9 | The size of the game download does not matter, because the LAN serves it. | Owner | Noted. Each client is about 51 MB, and the page shows the progress of the load (D5). |
| E10 | The rules of the game can be tested without a browser. | Proposed | Proven |
| E11 | The documents follow each change. The directions of the owner go into this document in clean words. | Owner | Proven in part: `feasibility.md` still describes the earlier checkout in places |
| E12 | One command, `cargo xtask serve`, builds the game and serves it, so that a person can review each change soon. | Owner | Proven |
| E13 | The game also runs in a browser without WebGPU, through WebGL2. The page then warns that the picture can be slower. | Owner | Proven. An earlier decision of 2026-11-21 said "WebGPU only". The owner asked for the fallback later that day, and the owner's tests on phones use it. |
| E14 | The game ships a large asset only in a reduced form. The terrain map of 153 MB becomes a height grid of 132 kB and a picture of 1.6 MB. | Owner | Proven |

## Security and conduct

| Id | Requirement | Source | Standing |
|---|---|---|---|
| S1 | The game uses no camera and no microphone. A design that isolates the camera passed its test, but no person owned the isolating page, and the owner ruled camera input out on 2026-11-21. | Note 16, owner | Disqualified: camera input |
| S2 | The gyroscope of a phone is allowed, because it reads no camera and no microphone. | Proposed | Proven |
| S3 | Commit messages carry no trailers and no co-author lines. | Owner | Proven |
| S4 | One long GPU job runs at a time on the development machine. | Owner's machine | Proven |
| S5 | The agents of the project work together through the issue tracker and the branches of the remote. Each agent reads both from time to time, and takes the opinions, the code, and the assets of the others into account. | Owner | Proven in part: this branch holds the documents, the audio, the planes, and the terrain of the others. Two of their decisions were overruled by later directions of the owner (E13, C2). |

## Disqualified ideas

These ideas were examined and ruled out. `feasibility.md` gives the evidence for the ideas from the notes.

| Idea | Source | Outcome |
|---|---|---|
| iroh for multiplayer | Note 2 | Works in a browser, and gains nothing on a LAN |
| Eye tracking for control | Notes 3, 7, 12, 13, 16 | A webcam is too coarse for aim, and S1 rules the camera out. |
| A relativistic game | Notes 7, 12, 15 | The rendering works. Nothing shows that it is fun, and it makes network delay hard to tell from light delay. It can return as a mode. |
| A soulslike, or a single boss fight | Notes 7, 10, 11 | Performance allows it. Its cost is animation content. |
| Google ML Kit for real objects | Note 6 | It supports no browser |
| Soft bodies and large destruction | Note 18 | They work. Planes need only a few rigid parts. |
| zig as a build tool | Owner | Forbidden |
| An inference backend | Note 5 | Too slow for combat. It can return for a line of text between rounds. |
| Assets that an agent generates through a paid service | Note 14 | Not tried. The planes and the terrain are generated models, and contributors made them (A7). |
| WebGPU only, with no WebGL2 build | Owner | Decided on 2026-11-21, and replaced the same day by E13, so that devices without WebGPU can play. WebGL2 is about 30% slower and has no compute. |
| Keys that steer the plane | Owner | Removed on 2026-11-21. Only the mouse steers on a desktop (C2). |
| The over-G alert of the cockpit voice | Owner | Removed on 2026-11-21 (A6) |
| Camera input for the view | Note 16, S1 | The isolation design passed its test, but no person owned the shell page. The owner ruled it out on 2026-11-21. |
| Art direction | Owner | Out of scope for the feasibility work |

## Open points

These points have no decision or no proof yet.

1. A playtest with people (P7, P9, P11, D2).
2. A test on real phones (C5, D1, D5): the stick, the gyroscope, full screen, and the load. Online play on an iPhone is not tested: Safari can refuse a WebSocket to a server with a self-signed certificate.
3. A test of the smooth motion on the owner's VPN (N4).
4. The licence of the plane models and of the terrain map (A7). The cockpit voice and the F-16 sounds come from projects under the GPL, which the licence of this repository must allow.
5. A person must listen to the sound (A6).
6. A full lobby of 16 real browsers (N8).
7. The show pieces of P6: replay, spectators, a graph of the delay, and one GPU simulation.
8. A battle royale rule set (note 17), as a stretch goal.
9. A choice between different maps, as a stretch goal of the owner.

Decided on 2026-11-21: camera input is out; the demo is hosted locally, so wired against wireless is moot; and the game runs with WebGPU, and with WebGL2 where WebGPU is missing.

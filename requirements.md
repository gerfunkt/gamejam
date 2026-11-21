# Requirements

This document collects what the project must do and must not do. It does not say how: `design.md` holds the design, and `feasibility.md` holds the evidence. It was last updated on 2026-11-21.

Each requirement names its source:

- **Notes**: a point of `game-notes.md`, with its number.
- **Owner**: a decision of the project owner during the work.
- **Proposed**: a proposal from the research, which the owner accepted or did not reject. The owner can overrule each one.

Each requirement has a standing:

- **Proven**: a measurement or an automatic match in a browser showed it.
- **Proven in part**: the note says which part is missing.
- **Not proven**: built or planned, and nothing has shown it yet.
- **Disqualified**: tried, and ruled out. The note says what replaced it.

## The product

| Id | Requirement | Source | Standing |
|---|---|---|---|
| P1 | The game is a 3D multiplayer shooter with planes, played in browsers on one LAN. | Owner | Proven |
| P2 | The view is third person. | Owner | Proven |
| P3 | The game has play against bots and play against people. | Owner | Proven |
| P4 | Each plane has a small number of parts that damage can break. | Owner, note 18 | Proven: a lost part changes the flight |
| P5 | The planes carry missiles as well as guns. | Owner | Proven |
| P6 | The game impresses judges through its mechanics: strong netcode, visibly high computation, and behaviour that is simple to describe. | Notes 3 and 4, owner | Proven in part: smooth motion and instant controls are proven. Replay, spectators, and a GPU simulation are not built. |
| P7 | The game is fun before it grows. A playtest proves the chase and the shot before content is added. | Proposed | Not proven: no playtest with people yet |
| P8 | Bots keep the sky busy when few people play. | Proposed | Proven |
| P9 | A player can see a missile coming and escape it, and a new plane cannot be killed at once. | Proposed | Proven in part: bots escape and survive in automatic matches. No person has judged it. |

## Controls

| Id | Requirement | Source | Standing |
|---|---|---|---|
| C1 | The controls are simple, with mouse and keyboard. | Owner | Proven |
| C2 | The mouse does most of the control. | Owner | Proven |
| C3 | The player can change each key in the settings. | Owner | Proven |
| C4 | Steering works in every current browser of the players. | Owner's test | Proven. Steering that captures the mouse pointer was disqualified, because it failed in the owner's browser. |
| C5 | A phone has its own controls: the gyroscope and touch. | Note 19, owner | Not proven: no real phone has run it |
| C6 | The game is complete with mouse and keyboard alone. Each other input is optional. | Proposed | Proven |

## The lobby

| Id | Requirement | Source | Standing |
|---|---|---|---|
| L1 | Solo play and online play each have a proper lobby. | Owner | Proven |
| L2 | The lobby sets the number of bots and their skill. | Owner | Proven |
| L3 | A player names itself. | Owner | Proven in part: letters with accents do not show correctly |
| L4 | A player starts its own lobby. | Owner | Proven |
| L5 | Other players find that lobby. | Owner | Proven |
| L6 | The host manages its lobby: the bots, the name, who stays, and when a match starts and ends. | Owner | Proven |
| L7 | When the host leaves, the lobby continues with a new host. | Proposed | Proven |

## The network

| Id | Requirement | Source | Standing |
|---|---|---|---|
| N1 | The players connect through one WebSocket message bus on the LAN host. | Owner | Proven. iroh was disqualified: in a browser it needs a relay, so it adds parts and gains nothing on a LAN. WebTransport was considered and is not needed. |
| N2 | The LAN host serves everything. The game needs no internet. | Owner | Proven |
| N3 | The game can be reached on every network of the host, a VPN included. | Owner's test | Proven |
| N4 | The planes of other players move on a smooth path over an uneven link, such as a VPN at 35 ms. | Owner's test | Proven in part: no step backward on a simulated link. The owner has not confirmed it on the real one. |
| N5 | The own plane and the own hits have no network delay. | Proposed | Proven |
| N6 | Every machine agrees on the shared events of a match. | Proposed | Proven |
| N7 | The server stays simple and knows nothing about the game. | Proposed | Proven. A server that decides the whole game was disqualified as too costly for a jam. |
| N8 | One lobby holds up to 16 players, and several lobbies run at once. | Proposed | Proven: 16 browser sockets in a room with the 17th refused (`4002`), and under generated load |
| N9 | The test and serve ports are 5201 to 5299. | Owner | Proven |

## Engineering

| Id | Requirement | Source | Standing |
|---|---|---|---|
| E1 | The game is Bevy in Rust, compiled to WebAssembly. | Note 1 | Proven |
| E2 | The build is pure Rust. It compiles no C code, and it uses no GCC, no clang, and no zig, for compiling or for linking. | Owner | Proven, with a check that records every call to a compiler |
| E3 | Very little code is JavaScript. Where the project needs it, the code is TypeScript. | Owner | Proven |
| E4 | The server is in Rust. | Owner | Proven. A JavaScript dev server was tried and disqualified. |
| E5 | mise manages the tools, and Mr. Boxington caches builds. No tool comes through a deprecated installer. | Owner | Proven |
| E6 | The speed of the game does not depend on the frame rate. | Owner's question | Proven |
| E7 | Claims about performance come from real runs. | Owner | Proven: 60 frames a second on a laptop with an integrated GPU, with room to spare |
| E8 | The loop of change, build, and test stays short, because contest time is short. | Owner | Proven: seconds for the rules, and under half a minute for the game |
| E9 | The size of the game download does not matter, because the LAN serves it. | Owner | Noted |
| E10 | The rules of the game can be tested without a browser. | Proposed | Proven |
| E11 | The documents follow each change. | Owner | Proven |

## Security and conduct

| Id | Requirement | Source | Standing |
|---|---|---|---|
| S1 | The game uses no camera and no microphone. A design that isolates the camera passed its test, but no person owned the isolating page, and the owner ruled camera input out on 2026-11-21. | Note 16, owner | Disqualified: camera input |
| S2 | The gyroscope of a phone is allowed, because it reads no camera and no microphone. | Proposed | Proven |
| S3 | Commit messages carry no trailers and no co-author lines. | Owner | Proven |
| S4 | One long GPU job runs at a time on the development machine. | Owner's machine | Proven |

## Disqualified ideas

These ideas from the notes were examined and ruled out. `feasibility.md` gives the evidence.

| Idea | Source | Outcome |
|---|---|---|
| iroh for multiplayer | Note 2 | Works in a browser, and gains nothing on a LAN |
| Eye tracking for control | Notes 3, 7, 12, 13, 16 | A webcam is too coarse for aim. Head pose for the view stays possible under S1. |
| A relativistic game | Notes 7, 12, 15 | The rendering works. Nothing shows that it is fun, and it makes network delay hard to tell from light delay. It can return as a mode. |
| A soulslike, or a single boss fight | Notes 7, 10, 11 | Performance allows it. Its cost is animation content. |
| Google ML Kit for real objects | Note 6 | It supports no browser |
| Soft bodies and large destruction | Note 18 | They work. Planes need only a few rigid parts. |
| zig as a build tool | Owner | Forbidden |
| An inference backend | Note 5 | Too slow for combat. It can return for a line of text between rounds. |
| Generated models and sound | Note 14 | Not tried. It waits until the game is fun. |
| WebGL2 as a fallback | Owner | Ruled out on 2026-11-21: WebGPU only, so the demo runs on Chromium. WebGL2 is about 30% slower and has no compute. |
| Camera input for the view | Note 16, S1 | The isolation design passed its test, but no person owned the shell page. The owner ruled it out on 2026-11-21. |
| Art direction | Owner | Out of scope for the feasibility work |

## Open points

These points have no decision or no proof yet.

1. A playtest with people (P7, P9).
2. A test on real phones (C5). The picture is also blurred there.
3. A test of the smooth motion on the owner's VPN (N4).
4. Names with accents (L3).
5. The show pieces of P6: replay, spectators, a graph of the delay, and one GPU simulation.
6. Sound and hit effects.
7. A battle royale rule set (note 17), as a stretch goal.

Decided on 2026-11-21: the game needs WebGPU; camera input is out; the demo is hosted locally, so wired against wireless is moot.

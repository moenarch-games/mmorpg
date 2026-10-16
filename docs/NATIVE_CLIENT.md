# Run the native multiplayer slice

The native client is a Rust executable using wgpu and winit. It connects to the existing zone host over TLS/WebTransport. It renders Greyhaven Vale from `mmorpg-scenery`: biome terrain, roads, shoreline, water, vegetation and the same procedural prop categories as the browser. Interpolated snapshots supply players, creatures and NPCs. Position and collision outcomes come from the server; units are drawn at their physics position plus shared presentation relief.

Native rendering consumes retained mesh, transform and material draw frames, with sRGB colors, hemisphere/sun lighting, fog and the browser's current opaque sky background. Its cosmetic day/night cycle has no authority over simulation. Ranger geometry comes from the authored OBJ package through external `tobj`; existing `3d-lab` mesh/math contracts remain, without making new internal tooling a prerequisite.

The Outpost grass family uses the [saved exclusion-mask package](SCATTER_AUTHORING.md) through shared scenery, retaining accepted transforms as shared immutable geometry with instance transforms.

The Outpost terrain and presentation heights consume the [saved relief field](RELIEF_AUTHORING.md) through the same shared scenery owner. Native smoke also renders its road/plaza/building-approach camera poses; `MMORPG_SMOKE_FRAME_DIR` saves their GPU frames.

## Prerequisites

Use the pinned Rust 1.98.0 toolchain. The native client needs a supported Vulkan, Metal, Direct3D or OpenGL backend and a desktop window system. Linux builds use the standard winit X11/Wayland dependencies; no GTK/WebKit installation is required. Offscreen GPU verification also works with a software Vulkan implementation.

The smoke helper additionally needs Python 3 and OpenSSL. Rust integration tests use local ephemeral TLS credentials and sockets, and do not require a GPU or display.

## One-command verification

```sh
python3 scripts/smoke-native.py --window
```

The helper builds locked sources, creates a disposable certificate/key, starts a real host on OS-selected ports, waits for readiness, connects the client, resumes the same player on a new connection epoch, and reads back a rendered GPU frame of the vale, which must contain several colours with no single colour (such as the sky) covering three quarters of it. It also runs the explicit GPU health-bar test: restore two authoritative fixtures differing only in one creature's health, project them through core, and require visible pixel changes. With `--window`, it then opens a native game window for 120 frames. It stops its processes and removes temporary credentials on success or failure.

## Interactive play

Start a local host and connect one native client with one command:

```sh
./scripts/dev-native.sh
```

The launcher requires Python 3, Cargo, Git, curl and OpenSSL. It fetches `origin`'s default branch and fast-forwards to the latest merged revision before building. A checkout that already includes that revision keeps its local edits and commits. If updating would require integrating local changes or a divergent branch, it stops with an explanation; it never stashes, resets, forces or changes dependency pins. `--no-update` explicitly uses the current checkout without fetching, including offline work.

It builds from its own checkout regardless of the calling directory, reuses a valid disposable certificate under `target/dev-tls/` (or creates one valid for 10 days), starts zone `1` at `https://localhost:4433`, waits for `/readyz`, and opens the client. It watches workspace source, shaders, embedded scenery and Ranger OBJ/manifest/material assets, Cargo manifests/lockfile, toolchain and `.cargo` configuration. An edit stops the owned host/client processes, rebuilds, then restarts them; edits during a build cause another build before launch. Failed builds remain visible and watching resumes after the next edit. Generated targets, logs and certificates do not trigger rebuilds. **Restarting resets the disposable zone and its development session.**

Closing the window or pressing Ctrl-C stops the owned host and its workers. `--no-watch` keeps a single interactive launch. It owns that host process, so do not use it while another local zone host is bound to ports `4433` or `8080`.

To exercise the exact launcher without leaving a window open:

```sh
./scripts/dev-native.sh --smoke
```

`--smoke` always runs once and cleans up. Add `--no-update` to verify the local checkout without fetching newer source:

```sh
./scripts/dev-native.sh --no-update --smoke
```

Browser tabs can join a local zone host too: `./scripts/dev-browser-online.sh` starts the host with the same certificate and ports and serves the page in [browser online mode](../README.md#browser-online-mode), which follows the session and resume rules below. Run it instead of this launcher, not beside it.

Run this command in another terminal for a second player after the launcher is ready:

```sh
cargo run --locked -p mmorpg-client -- --certificate target/dev-tls/cert.pem
```

To join the same host **from another computer on the LAN**, use the host's actual IPv4 address and transfer only the **public** `target/dev-tls/cert.pem` to that client (never `key.pem`):

```sh
cargo run --locked -p mmorpg-client -- \
  --url https://192.168.1.20:4433/game/matches/zone-1 \
  --certificate ./cert.pem
```

Replace `192.168.1.20` with the server machine's LAN address. The zone host already binds its WebTransport port for incoming peers; allow inbound **UDP 4433** from the LAN in the host firewall. Keep the launcher running (closing the host's local window stops its server). For friends who want to join in a browser without installing Rust, run `./scripts/dev-browser-online.sh --lan 192.168.1.20` **instead**; it serves an HTTPS game page and the same authoritative server. See [LAN browser hosting](../README.md#play-together-on-a-local-network) for TLS and firewall setup.

Controls:

| Input | Action |
| --- | --- |
| W / S (or ↑ / ↓) | Run forward / backpedal |
| A / D, Q / E (or ← / →) | Strafe left / right |
| Space | Jump (only from the ground) |
| Tab | Select the nearest living attackable creature; press again to cycle outward |
| F | Start auto-attacking the target, or stop |
| R | Release your spirit while dead |
| 1–4 | Use action-bar slot 1–4 (the class's four abilities) |
| Left click | Select that visible creature or NPC, without attacking |
| N / Enter | Cycle visible NPCs / open the selected person’s context |
| Context: Up / Down | Browse every quest, stock offer and bag slot |
| Context: 1 / 2 | Accept/complete the displayed quest or buy one displayed item / sell one from the displayed bag slot |
| Context: Left / Right | Choose a quest reward |
| Quest context: H | Page through the authored dialogue and named objectives |
| Left or right mouse drag | Orbit the camera around your character (five-pixel click threshold) |
| Mouse wheel | Zoom the camera |
| Escape | Close person context or cancel an armed click; otherwise cancel a cast/channel or close |

The third-person camera follows your chosen class/sex appearance. Its first view chooses an unobstructed heading from the same ordered candidates as the browser; subsequent projections preserve user orbit. The eye stays above the rendered terrain under it. While a movement key is held, your character turns to face the camera's direction, so W always runs away from the camera; releasing the keys leaves the character facing where it last moved. Running and strafing move at 6.3 m/s, backpedalling is slower. Losing focus clears held movement, and so does dying: while dead the client sends no held movement or jumps, and held keys take effect again once a projection shows you alive. Movement is sent on change and retransmitted at 20 Hz so a lost key-release datagram is corrected; each Space, Tab, F, R, 1–4 or cast-cancelling Escape press sends one command and is never replayed after a reconnect. `--class warden|ranger|arcanist` and `--sex female|male` (a male Warden by default) choose the character; the session sends `ChooseClass` with every heartbeat, under fresh sequences, until a projection shows a class (so a lost datagram cannot leave the character classless, also across reconnects), keys 1–4 use the class's four abilities at the current selection, and Escape cancels a running cast instead of closing the window. Keys 1–4 and the cast-cancelling Escape resolve through the shared `input-bindings-core` runtime (`controls.rs`): the same semantic actions, contexts and default keys as the browser (`ability.slot1`–`4` in `gameplay`, `combat.cancelCast` in a non-blocking `casting` layer pushed while the latest projection shows a cast), one use per press, no OS key repeat, and focus loss releases every held action. Holding Escape never closes the window; only a fresh press with no context, armed pointer or cast does. A minimal combat HUD (`hud.rs`) above the bottom edge shows the projected class resource bar, the cast bar (a channel drains) and the four slots with their cooldown or global cooldown, read only from the received projection and the ability catalog. The title shows the class resource and cast progress.

Units use the browser's current class/sex looks, headwear variants, creature families and NPC-role models. Rangers use the normalized authored archer mesh; other humanoids and creatures use procedural mesh parts. Movement, jump/turn, viewer ability poses and spell effects derive from presentation time and received state. Reduced motion holds rest poses and suppresses transient animation. Living players and creatures retain camera-aligned health bars based on projected health; NPCs and corpses omit them. Target markers, unit names and class/resource/cast information remain presentation of authoritative projections. There is no local movement prediction, so input response includes network and interpolation delay.

The default endpoint is `https://localhost:4433/game/matches/zone-1`. For another configured zone:

```sh
cargo run --locked -p mmorpg-client -- \
  --url https://localhost:4433/game/matches/zone-2 --zone 2 \
  --certificate target/dev-tls/cert.pem
```

For publicly trusted server certificates, omit `--certificate`; system certificate trust is then used. The client never disables certificate verification. `--smoke` connects, resumes the same session, and verifies a single offscreen GPU frame; `--frames N` limits a window run for lifecycle checks. Invalid configuration, incompatible tick rates/content, and failed connections exit nonzero.

## Connection interruptions

While the window remains open, a transport timeout/local connection loss or five seconds without advancing snapshots triggers one resume attempt. The title shows “reconnecting to world” while the last known scene remains visible. Resume uses the same TLS configuration and endpoint, keeps the same player, retains command sequencing, and resets interpolation. It sends a stopped movement intent (keeping the last facing) and waits for acknowledgement before sending the current held input again. Jump presses made during the interruption are dropped.

Tokens stay in memory and never appear in CLI arguments or application logs. Session URLs must name a fresh hosted-match route without credentials, query parameters, or fragments. The server owns token rotation and expiry. Reconnect lasts at most ten seconds or the advertised grace period, whichever is shorter; it includes a 250 ms delay for server-side disconnect processing. That delay is best effort, not an acknowledgement protocol. The retained player may continue its last server-owned movement during the interruption until the stop reaches the server.

A rejected, expired, incompatible, or incomplete resume ends the session with an error. The client never substitutes a new player. The browser's online mode implements the same resume rules; browsers cannot tell a server's application close from a lost path, so it treats both as an interruption and relies on the host refusing a resume it will not grant. Protocol errors and server application rejections are terminal. Closing the window cancels a pending attempt. Resume after closing the application, server restart recovery, and live zone rerouting are not implemented.

## Current limits

- This is a connected gameplay/graphics slice, not a production account system. Sessions are anonymous; account/character binding, persistent resume across application launches, and live zone handoff are pending.
- The host remains standalone; do not run competing hosts for one zone without the planned distributed lease integration.
- World/model/environment parity acceptance is documented in [NATIVE_WORLD_PARITY.md](acceptance/NATIVE_WORLD_PARITY.md); paired rendered review and native application checks are required before release. Native world clicking and selected-person quest/vendor interactions are available; context text and paged dialogue use the window title. Classes, resources, cast/cooldown HUD and world-space health bars are implemented. Rich target frames, nameplates, combat text and a general bag window remain pending; merchant bag-slot navigation uses received self sheets and shared authoritative commands.
- The shared transport fragments oversized session snapshots and the client reassembles them per connection. The current v15 MMO projection policy still keeps at most 64 relevant units and packs as many 21-byte records as fit 1,077 bytes (46 without events, sheets, cooldowns or auras, 38 beside the largest self sheet); additional projected sections require a separate protocol and budget change.
- The built-in Greyhaven Vale (content revision 9, with creatures, NPCs, items and equipment, loot rules, creature abilities, a vendor and the quest chain) and snapshot v15 change the standalone host's initial simulation state. Recovery bundles captured with earlier content or snapshot versions cannot be silently reused; arrange an explicit migration or fresh development state.
- Linux is the exercised desktop platform in this change. Windows/macOS builds and installers remain unverified.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo build --workspace --all-features --locked
python3 scripts/smoke-native.py --window
```

The live convention stack for this change resolved to sourceRevision `e6acb5310afaf15c0cba24f87108f5f4ad1bedc3`. No existing locked package identity was removed during native dependency acquisition.

Native resume was implemented from repository baseline `80acad9d0d810fd048019fd824d42a1633774953`. At that baseline the shared `game-server` pin was `769de47005cc37891011fc76ae183c18b7c5e0ae`; the current pin is listed in the README. The client now directly declares the already-locked `url` 2.5.8 parser; no locked package version changed.

Health-bar slice (#71): rendering keeps the existing instanced-box path with at
most 16 model boxes and two bar boxes per admitted unit plus one target marker
(1153 at the core projection cap of 64; `MAX_SCENE_BOXES`). Oversized presentation input fails before history changes.
No gameplay, projection bytes or scenery identity changes. The optional
`MMORPG_SMOKE_FRAME_DIR` environment variable saves the full/damaged GPU fixtures
as PPM files during native smoke. These are test artifacts, not gameplay state.
Resolved conventions sourceRevision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.

`ClientSession::send_loot` carries a projected corpse's creature ID/death tick
through the shared command transport. It owns no eligibility or settlement rules.
A real two-client acceptance test refuses a non-owner, credits the owner once,
resumes the session and refuses a duplicate without changing copper or the bag.
This API does not add a native loot-window control.

Native world selection queries the last drawn model parts and the uploaded terrain, water and prop geometry through pinned `3d-lab::three_d_projective::picking`. The client retains EntityRef ownership; health bars and target markers are not selectable bodies. NPC context uses existing content and received quest/bag sheets, advertises navigation in the window title, and sends existing quest/vendor commands only after a deliberate action. The zone validates state, reach and settlement; refusals remain visible in the context. Focus/leave/cancel retires armed clicks through the shared input runtime; reconnect retires one-shot interactions along with the existing combat presses.

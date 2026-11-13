# Run the native multiplayer slice

The native client is a Rust executable using wgpu and winit. It connects to the existing zone host over TLS/WebTransport. It renders Greyhaven Vale from `mmorpg-scenery`: biome terrain, roads, shoreline, water, vegetation and the same procedural prop categories as the browser. Interpolated snapshots supply players, creatures and NPCs. Position and collision outcomes come from the server; units are drawn at their physics position plus shared presentation relief.

Native rendering draws the retained mesh, transform and material draw frames with [render-engine](https://github.com/moritzbrantner/render-engine) (#529), pinned by git rev and measured against Bevy in its own repository. Static scenery and props are uploaded once into its `World`, and a frame shares them as `DrawBatch`es that the renderer resolves into instances once and reuses until the batch is dropped (#588); a drawn batch and each drawn tree or rock level are render-engine retained instances (#728), added when first drawn and removed when no longer drawn, so a steady frame writes no scenery instances; only the per-frame draws (units, effects, animated props, door swings) are resolved each frame. Skinned units are skinned on the GPU (#595): each imported model's primitives are uploaded once in their bind pose with joints and weights and shared by every unit of that model, each unit has one render-engine skin, and a frame uploads only its joint palette. The zone's clock gives the hour: render-engine's `DayCycle` places the sun (cascaded shadows) and the moon, its physically based sky follows them and lights the scene through its filtered environment light, and the frame is exposed by sun elevation and tone mapped (Tony McMapface). Distance fog keeps the browser's palette. Campfires, lamp posts and the market's lantern post are point lights (`mmorpg_presentation::lights`), and flames, lamp glass and lit windows glow. The combat HUD and the egui interface are drawn over the tone-mapped frame. The browser lights the same way through render-engine's three.js port `@moritzbrantner/three-lighting` (#600, `web/src/world/lighting.ts`): the same `DayCycle` sun and moon, atmosphere sky and its environment light, exposure, tone mapping, fog and point lights, with three sun cascades instead of four over the same 160 m (three.js redraws the scene per cascade). Only where the browser lacks `OES_texture_float_linear` or `EXT_color_buffer_float`, and on the character-selection stage, the CSS sky shows through a transparent canvas, lit by one directional light and an ambient sky term tuned to the native frames. The cosmetic day/night cycle has no authority over simulation. Ranger geometry comes from the authored OBJ package through external `tobj`; existing `3d-lab` mesh/math contracts (geometry, the CPU camera and picking) remain, without making new internal tooling a prerequisite.

Default lighting numbers (`crates/mmorpg-client/src/graphics/frame.rs`): sun 130 000 lx above the atmosphere (render-engine's `RAW_SUNLIGHT`, dimmed and reddened by the sky), 60° at noon, rising at 06:00 and setting at 18:00; moon 2 lx, opposite the sun; EV100 15 at a 60° sun, 13.8 at 15°, 11 at 2°, 6 at −4° and 2 at −12° or below; sky environment light at intensity 0.65 in daylight, returning smoothly to 0.9 at/below the horizon (no separate ambient); fully adapted cave interiors retain intensity 0.1; four sun cascades from 0.2 m, the first to 12 m, out to 160 m, none below −3°; emissive colours at 80 cd/m²; campfires 1 800 lm (range 16 m, flickering), lamps 900 lm (range 12 m), unshadowed, assigned to view-space clusters so a fragment shades only its cluster's lights (render-engine's default since render-engine#94, #725; the per-fragment loop where the device lacks the storage buffers); aerial perspective as atmospheric fog (#526): contrast falls to 5 % at 6 km, extinction weighted 1.1/1/0.9 and inscattering 0.9/1/1.1 per red/green/blue, so distant ranges turn blue but keep their shading.

**GPU particles** (#769, Atlas 6.3.i769) use render-engine's emitters in the same `World` and frame (`crates/mmorpg-client/src/graphics/particles.rs`); render-engine simulates and draws them, the client only decides where and when. Every Greyhaven campfire (the flickering campfire point lights, embers 0.45 m below) gets additive flame licks, sparks streaking along their velocity and lit, fogged, soft-edged smoke; every still water body (Stillwater Lake and the marsh pools) gets a few faint, lit mist wisps lying on its surface, separate from the distance fog. A zone-confirmed hit (`DamageDealt`, `DamageTaken`, `DamageObserved`) or block (`Blocked`) bursts the browser's authored asset-tooling puff or ring sequence (#52, `web/public/effects`) at the struck unit's presented body centre, with smaller copies thrown along the blow from the striker; the trimmed atlas frames are repacked once into a flipbook that keeps the authored per-frame timing. Each projection's impacts are admitted once, when the presentation accepts it as newer, and are dropped when the struck unit is not presented; a striker the viewer cannot see only loses the direction. The soft sprite and the four impact flipbooks are uploaded once per renderer; environment emitters live as long as their scene and are replaced, never accumulated, when the scenery changes; the six impact emitters spawn only by bursts and are bounded by their capacity (32 sequences per kind and intensity). Particles step at a fixed 60 Hz on the client's presentation clock (at most three steps per frame), never zone state. Reduced motion retires the campfire and mist emitters and plays impacts with the authored subtle sequences in place, without the spray. Exclusions: no precipitation or weather (the scenery and zone have no weather data, and none is invented), no particle editor, browser WebGPU particles, trails or collisions; the browser's #52 sprites are unchanged and particles never feed back into gameplay, combat timing or the network.

Particle evidence: `MMORPG_PARTICLE_SHOTS=<dir> cargo test --release -p mmorpg-client --lib greyhaven_particle_shots -- --ignored --nocapture` writes a campfire by day and night after five seconds of GPU simulation (fast-forwarded without redrawing the scene, and checked against a frame without particles), mist on Stillwater Lake and hit and block bursts from a gameplay camera; `cargo test --release -p mmorpg-client --lib greyhaven_particle_lifetime -- --ignored --nocapture` checks camera movement, emitter replacement on a new scene, repeated bursts staying within capacity and dying out, and reduced motion. `outpost_frame_time` (below) reports the CPU particle step and, with GPU timestamps, the `particle_sim` and `particles` passes; `MMORPG_PROFILE_NO_PARTICLES=1` disables the particles for an A/B and `MMORPG_PROFILE_FRAMES=<n>` (at least 8) shortens the default 240 frames per view. On an RTX 3060 Ti at 1280×720 the hub view measured a 5.91 ms median GPU frame with particles (`particle_sim` 0.02 ms, `particles` 0.05 ms) against 5.83 ms without (advisory, not a gate).

Lighting evidence: `MMORPG_LIGHTING_SHOTS=<dir> cargo test --release -p mmorpg-client --lib greyhaven_lighting_shots -- --ignored` writes the Outpost at noon, dusk and night and a campfire and the lantern post by night; `cargo test --release -p mmorpg-client --lib outpost_frame_time -- --ignored --nocapture` prints the advisory frame time of six Greyhaven views: the debug viewpoints `hub`, `woods`, `vale` and `lake` (#692), `starter`, the camera a new character gets at spawn, and `crowd`, 64 players (the projection's unit cap) of every class on the hub plaza (#724), and `MMORPG_PROFILE_VIEW=<name>` times one of them. Per view it prints the median, p95, p99, maximum and first frame of the wall time, render-engine's CPU `prepare` and `encode` and the GPU time (timestamp queries, where the adapter has `TIMESTAMP_QUERY`), the median GPU time per pass, `Viewport::work()` and the `World::work()` uploads over the view, and the raw per-frame lists including the first frames. It is a manual measurement, not a gate.

The Outpost grass family uses the [saved exclusion-mask package](SCATTER_AUTHORING.md) through shared scenery, retaining accepted transforms as shared immutable geometry with instance transforms.

The Outpost terrain and presentation heights consume the [saved relief field](RELIEF_AUTHORING.md) through the same shared scenery owner. Native smoke also renders its road/plaza/building-approach camera poses; `MMORPG_SMOKE_FRAME_DIR` saves their GPU frames.

## Host World

Build/install both native binaries together (`cargo build --locked -p mmorpg-client -p mmorpg-game-server`), then start `mmorpg-client` without `--class`. On the character screen, create or select a character and choose **Host World**. The game starts/reuses the sibling `mmorpg-zone-host`, waits for readiness and connects the owner through authenticated loopback WebTransport using its automatically generated certificate. The game-owned save is `<native configuration directory>/worlds/default/`: on Linux, `$XDG_CONFIG_HOME/mmorpg/worlds/default` or `~/.config/mmorpg/worlds/default`. `MMORPG_CONFIG_DIR` selects an isolated configuration directory and roster. Managed hosting serves Greyhaven (zone 1).

The host registers through moenarch.com's existing lobby service once ready. **Copy invitation** appears only after successful registration, on character selection or in the Escape game menu. Sharing errors hide the invitation and offer **Retry sharing**. Local owner play continues if registration/signaling is unavailable. Invitations use the deployed [native landing page](https://moenarch.com/native.html); that page supplies instructions and public lobby metadata, not browser gameplay or proof that a host is healthy.

Guests create/select a local character, paste the complete invitation and choose **Join World**, or run:

```sh
cargo run --locked -p mmorpg-client -- --join-invite 'https://moenarch.com/native.html?lobbyId=0123456789AB&matchId=zone-1'
```

The example IDs above are illustrative; use the actual host's link. CLI joining opens character selection with the invitation filled in. The host approves the guest's displayed code in the Escape menu before that guest enters gameplay. A valid returning guest resumes their own saved character with the same local roster slot/key; select its original class. **Remove** disconnects a guest and denies further access while retaining progression. **Approve** can restore that same entitlement later. The local `guest-identities/` keys are private game credentials: retain them with the guest's configuration directory. Setup capabilities, admission tickets, TURN credentials and reconnect tokens are never stored there or copied into invitations. See [the admission decision](adr/0011-native-hosted-guest-admission.md).

The world keeps running when the client closes or crashes. **Stop World**, available on character selection and the game menu, disconnects everyone and waits for the existing final world/character save. No boot/login service or crash restart is installed. `characters/` and `recovery/` remain authoritative; incompatible recovery fails with diagnostics in `host.log`, never a silent reset. The OS-held `world.lock` spans all writers and runtime teardown; do not delete it to force another host. The private per-run certificate, key and discovery/control capability are temporary. Owner control listens only on loopback and authenticates every action. The native owner's gameplay listener also binds loopback; remote guests enter through authenticated WebRTC with direct-first ICE and configured TURN fallback.

The setup service retains its bounded lobby lifetime and renewal cap. Sharing expiry/loss clears availability; the owner can retry sharing and supply a fresh link. Expired/stopped-host links fail native admission with an actionable error; public setup metadata may remain until expiry. A transient connection interruption resumes through the existing rotating token and fenced epoch; a fresh client after host restart may wait for the prior offline character lease's normal reconnect grace. A failure before entry leaves character selection available for retry; terminal failure after connection closes the window with an explicit reopen instruction, rather than offering inactive entry controls. No guest may choose another character's durable ID. **Enter World** retains the manually configured `--url` / `--certificate` development/LAN path.

This delivers [#733 stage 5](https://github.com/moenarch-games/mmorpg/issues/733); #733 remains open for immutable manifest/missing-asset streaming and full local/two-network acceptance. `python3 scripts/smoke-hosted-native.py` is an opt-in real-process stage-5 check, requiring adjacent `multiplayer-setup-service` and coturn (`turnserver`, or `MMORPG_TEST_TURNSERVER`). It uses disposable saves/guest keys and loopback setup/TURN processes; `MMORPG_TEST_SETUP_ENDPOINT` accepts only loopback HTTP for this explicit harness, never an endpoint from an invitation. Native window interaction, Windows/macOS process/key permissions and real-network MMORPG NAT/TURN remain unverified by this local check.

## Optional hosted world detail

The native binary embeds essential playable content and the dog/rodent, but **does not embed the Greyhaven cat's glTF or geometry**. The cat is optional cosmetic detail with no collision, unit identity or simulation authority. Package a built game with `python3 scripts/package-native.py DESTINATION`; both binaries stay together, and the host's approved raw art is installed at `assets/ambient`. `--without-optional-art` produces an installation that genuinely lacks those external files. Source development may explicitly set `MMORPG_SHARED_ART_DIR` to the acquired ambient bundle directory; there is no automatic checkout lookup. The managed child inherits this install-path setting, while development stores/routes remain excluded.

A ready native host verifies the asset-tooling `gltf-resources-v1` manifest and the cat's immutable glTF/resource closure once, then serves only those hashes from memory over the admitted peer's optional reliable content lane. The manifest binds the core content revision and canonical bundle JSON SHA-256. Native clients require the same supported art-release identity; a mismatched world/release or invalid hash never reaches the importer. No bulk bytes travel through the site or setup service. Transport admission, connection epochs, revocation and host shutdown fence the content lane alongside gameplay.

The window keeps sending movement and receiving authoritative projections while detail loads on an independent content handle. Verified bytes enter the existing `bundle_glb` / `Model::parse` / animated draw path, with no alternate parser or renderer. The content-addressed cache lives under the game configuration directory at `asset-cache/sha256`; every read is hash-verified. A corrupt cache entry is discarded and fetched again, and a corrupt download is rejected before atomic installation or import. Subsequent joins reuse verified files without requesting their bytes. Optional loading errors appear in the game's system log with retry/install guidance; the world remains playable. An invited client never falls back to locally installed art when the host lacks the capability.

The intentional game-server pin is `d5f3c6ef54a73b70e442247f415d331e51f6a505` (merged content lane, retaining managed attachment and explicit owner-loopback binding). Content requests are opaque application messages on an opt-in capability; existing peers without it retain gameplay and receive an actionable optional-detail error. No zone command, projection, observation or canonical format changed, so this adds no `PROTOCOL_ID` rotation. The existing fixed request bounds and game-owned hash closure do not create another registry or authoritative runtime.

`python3 scripts/smoke-hosted-native.py --assets` is the opt-in local real-process check using the **shipped native executable's** `--smoke --join-invite` route, isolated empty guest cache and asset-free guest installation. It exercises cold load during movement, actual offscreen GPU consumption, warm reuse, corrupt-cache refetch and revoked admission. `MMORPG_SMOKE_FRAME=FILE.png` saves the native offscreen frame. This command does not itself prove character-screen pointer actions or traversal between separate networks; those requirements remain part of #733's final acceptance.

## Prerequisites

Use the current stable Rust toolchain from rust-toolchain.toml (minimum supported Rust 1.98). The native client needs a supported Vulkan, Metal, Direct3D or OpenGL backend and a desktop window system. Linux builds use the standard winit X11/Wayland dependencies; no GTK/WebKit installation is required. Sound (kira on cpal) needs the ALSA development headers (`libasound2-dev`) to build; without an output device the client stays silent. Offscreen GPU verification also works with a software Vulkan implementation.

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

The launcher requires Python 3, Cargo, Git, curl and OpenSSL. It fetches `origin`'s default branch and fast-forwards to the latest merged revision before building. A checkout that already includes that revision keeps its local edits and commits. If updating would require integrating local changes or a divergent branch, it stops with an explanation; it never stashes, resets, forces or changes dependency pins. `--no-update` explicitly uses the current checkout without fetching Git source, including offline work with already prepared assets.

On first use, or when cached native bundles or the Ranger class presets are missing or incompatible, the launcher runs `scripts/fetch-bundles.sh` and `scripts/prepare-ranger-native.sh` before Cargo. This asset-acquisition phase needs network access, Bun and the native preparation tools; once the cached inputs are valid, later launches skip it. This also repairs a stale or corrupted Ranger appearance descriptor that would otherwise make the native build panic at `appearance presets`. Direct `cargo build` still requires the assets to have been prepared explicitly.

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
| A / D (or ← / →) | Strafe left / right |
| Space | Jump (only from the ground) |
| F | Dodge roll along the held movement, or straight back standing still (#516; moved off Shift by #294) |
| X | Swim down while held (#274); held Space swims up |
| Tab | Select the nearest living attackable creature; press again to cycle outward |
| R | Release your spirit while dead |
| 1–9, 0 | Use action-bar slot 1–10 (the class kit in catalog order; the Arcanist's Ice Wall and Earth Pillar take 9 and 0); offensive abilities fire where the character faces, heals and buffs land on the selected friend or yourself |
| Shift+1–9, Shift+0 | The second action-bar row: slots 11–20, for class kits that outgrow the first ten (#294) |
| Q | Swing immediately where the character faces (#517, #636), with or without selection |
| G (hold) | A Warden's directed Heroic Strike: mouse motion picks side and height, release strikes (#154, #636) |
| Z (hold) | A Warden's shield guard: mouse motion picks the lane (#156) |
| T / K (hold) | With Settings → Wheels on: the fixed / context radial wheel; flick toward a sector and release, or tap and left-click; right click cancels ([controls](CONTROLS.md#radial-wheels-as-delivered-653), #653) |
| Left click | Select at the centre reticle in captured free look, or under the visible cursor while focused (a focused click on empty world clears the focus); does nothing during a held strike or guard; when free look has been released, the first world click recaptures instead (#636) |
| Right click | Cancel a held strike or guard without firing, otherwise cancel the cast (#636) |
| N | Cycle visible NPCs |
| E | Interact: open the selected NPC’s context (quest/vendor window); with no NPC selected, use the door or repair the structure in reach |
| Enter | Focus chat |
| B / C / L | Toggle bags / character / quest log |
| Chat field | Enter sends (`/s`, `/y`, `/wave`, `/bow`, `/cheer`, `/laugh`, `/point`), Escape returns to the world |
| Interface clicks | Action-bar slots, Talk/Loot/Clear on the target frame, quest Accept/Complete with reward choice, Buy/Sell, bag move/split/merge/Equip, Unequip, Take all, Abandon, Release spirit |
| Context: Up / Down | Browse every quest, stock offer and bag slot |
| Context: 1 / 2 | Accept/complete the displayed quest or buy one displayed item / sell one from the displayed bag slot |
| Context: Left / Right | Choose a quest reward |
| Quest context: H | Page through the authored dialogue and named objectives |
| Free-look mouse motion | Captured pointer steers the over-the-shoulder camera and facing; unfocused A/D remain strafes |
| Focused enemy | Camera and facing track the chosen living opponent; cursor is free to click action-bar/UI, A/D circle it |
| Mouse wheel | Zoom the camera |
| Escape | Close a context or open window first; otherwise open the game menu (Resume, Settings, Quit game) (#636) |
| Strike or guard held | One mouse owner at a time (`mmorpg-client-model`, #636): motion aims the gesture; the Settings choice "Camera during a strike or guard" keeps the camera locked (default) or free |

The shoulder camera follows your chosen appearance. Its first view chooses an unobstructed heading as in the browser and stays above terrain. While the mouse is captured in free look, camera heading drives facing even at rest; a selected living enemy instead drives smooth camera tracking and facing. A/D always strafe so the character can circle the opponent. Releasing focus keeps the previous heading until new mouse input. Running and strafing move at 6.3 m/s, backpedalling is slower. Losing focus clears held movement, and so does dying: while dead the client sends no held movement or jumps, and held keys take effect again once a projection shows you alive. Movement is sent on change and retransmitted at 20 Hz so a lost key-release datagram is corrected; each Space, Tab, F, Q, R or 1–4 press (or right click while casting) sends one command and is never replayed after a reconnect. Without `--class`, the window opens on the local character roster on its own selection stage (`stage.rs`: a floor, columns and a backdrop around the previewed character, under a fixed close camera) and neither builds the zone nor connects until you enter the world: Enter World starts building the zone's scenery and world on a background thread while the session connects and claims, once the first projection arrives that thread also composes one frame of the zone around the viewer (importing the trees, rocks, ground, buildings and unit models it draws and lowering the nearby static scenery), and the stage keeps drawing ("Entering the world…") through all of it. The first projection then enters the zone behind the stage: each stage frame uploads up to 8 ms of the zone's first frame into render-engine's `World` (`WindowRenderer::preload`), and the stage leaves once all of it is resident (#726). On an RTX 3060 Ti in a release build the longest stage frame during entry was about 120 ms (one 4096² tree texture inside render-engine's `World::add_texture`), where the first zone frame used to stall for about 3.5 s; `--class` and `--smoke` still pay that first frame at once. Pick a character or create one (name, class, body and headwear, with the browser's rules: 2–24 letters, unique names, at most 8 slots, lowest free `local-N` ID), change its headwear or delete it, and turn your avatar on a turntable preview (±15° steps, slider, Reset view) before entering. The roster is local presentation choice only, kept as versioned JSON in `roster.json` under `$MMORPG_CONFIG_DIR` or the platform configuration directory (`$XDG_CONFIG_HOME`/`~/.config/mmorpg`, `~/Library/Application Support/mmorpg`, `%APPDATA%\mmorpg`); an invalid file fails closed and is never overwritten, and no world progress is stored. Entering sends the existing `ChooseClass`; `--class warden|ranger|arcanist` and `--sex female|male` (male by default) skip it; the session sends `ChooseClass` with every heartbeat, under fresh sequences, until a projection shows a class (so a lost datagram cannot leave the character classless, also across reconnects), keys 1–0 use the class kit at the current selection, and right click cancels a running cast (#636). Keys 1–0 resolve through the shared `input-bindings-core` runtime (`controls.rs`): the same semantic actions, contexts and default keys as the browser (`ability.slot1`–`10` in `gameplay`, `combat.cancelCast` in a non-blocking `casting` layer pushed while the latest projection shows a cast), one use per press, no OS key repeat, and focus loss releases every held action. Escape never closes the window: it closes a context or window, otherwise opens the game menu, whose Quit game does (#636). The interface (`ui.rs`, #183) composes external [egui](https://crates.io/crates/egui) 0.37 through `egui-winit` and `egui-wgpu`, painted into the resolved frame after the world. It shows unit and target frames (health, class resource, XP, copper, auras, target cast and target-of-target), the cast bar and clickable action bar with cooldown/global-cooldown shading, a refusal banner, chat and combat logs, a rotating minimap with roads, water, buildings, unit dots and quest markers, the quest tracker and log, NPC quest dialogue and vendor windows, bags, the character pane, corpse loot and the dead state. Behind the windows, world-space nameplates (disposition colour, larger for the target) and NPC quest `!`/`?` markers hang above each presented unit's posed model head, and floating combat text (damage, criticals, periodic ticks, damage taken, heals, Absorb, Miss/Evade/Whiff, interrupts — the browser's `combat-text.ts` rules) rises over the unit each projection event concerns. Everything is read from the received projection, the retained self sheets and immutable content; each control sends an existing shared command and the zone validates it. egui receives pointer events always but the keyboard only while the chat field is focused: a press on a panel never reaches the world (releases always do), focusing chat retires held movement and actions, and focused buttons never swallow movement keys. B, C, L and Enter resolve through the same `input-bindings-core` catalog as the browser (`ui.toggleBags`, `ui.toggleCharacter`, `ui.toggleQuestLog`, `ui.openChat`). E (`world.interact`) opens the selected NPC's context and Enter only opens chat (#515). The title still shows status and context text.

A standard gamepad plays the window like Pages (`gamepad.rs`). Devices come from external [gilrs](https://crates.io/crates/gilrs) 0.11, through the `gilrs-azul` 0.11.3 fork, whose only differences are loading libudev at run time (no libudev development headers needed to build; without libudev there are simply no pads) and an unused serial accessor. Pads are sampled once per frame in W3C standard mapping and resolve through the same `input-bindings-core` runtime with the browser's pad catalog (`web/src/input/game-controls.ts`: same action ids, button/axis indices, 50 % threshold and 20 % axis deadzone hysteresis): the left stick runs and strafes and feeds the same held-movement path as W/A/S/D (the camera-facing rule included), A jumps, X swings the weapon (#517), Y releases the spirit, RB targets the next unit, the d-pad uses slots 1–4 (up, right, down, left), Select toggles bags and B dodge-rolls (#516; while a cast runs it cancels the cast instead). Natively the right stick orbits the camera at an analog rate, LB cycles NPCs, Start talks to the selected NPC, L3 toggles the character pane and RT the quest log. Holding LT raises the Warden's shield guard (#156): the right stick's vertical picks the lane (up high, centred middle, down low) and the camera stops tilting meanwhile; release lowers it, as in Pages. While a window is open (bags, character, quest log, loot, an NPC context or character selection), a non-blocking `panels` layer gives B "close", the d-pad Shift+Tab/Tab focus movement and A Enter on the focused egui control; movement and other gameplay buttons still fall through. B never leaves the world: the native window has no character screen to return to. Window focus loss, a pad disconnect and every context change retire held pad actions (a still-deflected stick resumes, a still-held button does not fire twice), and pads are ignored while unfocused or typing.

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
- Players are asset-tooling's Ranger humanoid (Atlas 6.3.7) and wolves, vermin and boars Quaternius CC0 animals (`assets/quaternius/*.glb`, Atlas 6.3.6); humanoid creatures and NPCs keep procedural models natively (Pages draws the named Greyhaven residents in their asset-tooling appearance presets, #418; the native client does not load those presets yet). `scripts/fetch-bundles.sh` downloads the Ranger bundle into ignored `web/public/characters/`; `scripts/prepare-ranger-native.sh` prepares standard GLBs with asset-tooling; the client's `build.rs` embeds them and the shared animation library and fails with that hint while it is missing. Models are imported once with the `gltf` crate (prepared standard GLBs; shared animation targets bind by joint name), base colours and base-colour texels become vertex colours, and `presentation/skinned.rs` samples idle/walk/run/attack/death clips into a joint palette per unit, drawn over the shared bind-pose meshes and skinned on the GPU; items and plates at rest are static meshes baked once, and picking poses a clicked body on the CPU so it still hits the posed mesh. Every class and sex shares the Ranger figure until more character bundles exist, each class in its starter outfit (asset-tooling #379: the `class.<id>` preset's outfit variant, its armour plates and its `classTrim` in the class accent colour; the outfit's own hood and head plate under the `class` headwear; the viewer's equipped head, chest, legs and feet drops replace the outfit's plates on those sockets with the knight's plate while the trimmed garments stay). A Warden also draws the bundle's sword and shield, a Ranger its stowed bow and quiver, a Rogue the blackened dagger, the Arcanist the crystal staff and tome, the Cleric the flanged mace and the Necromancer and Druid their bone and gnarled staffs (asset-tooling #375, #379) at the `humanoid-sockets` sockets (placement × joint × socket each frame), and a standing Warden holding guard plays `Idle_Shield_Loop` (no layered upper-body blend: walking keeps the walk clip, and only the viewer's own guard is projected); models are cosmetic and never change bodies or hits. Pages presents the same models with plain three.js (`GLTFLoader`, `AnimationMixer`, socket parenting; `web/src/world/units/quaternius-units.ts`), where other players also wear their preset's face, hair and beards (native draws the starter hair and no beards).
- World/model/environment parity acceptance is documented in [NATIVE_WORLD_PARITY.md](acceptance/NATIVE_WORLD_PARITY.md); paired rendered review and native application checks are required before release. Native world clicking, selected-person quest/vendor windows, unit/target frames, action bar, bags, character pane, loot, chat, quest tracker/log and minimap are implemented with egui (#183); the keyboard context and title text remain. The local character roster with appearance persistence and turntable preview, floating combat text and world-space nameplates/quest markers are implemented (#183). Gamepad access is implemented (#183). The Settings menu and character-scoped rebinding are implemented; see [client Settings](#client-settings-and-character-controls).
- The shared transport fragments oversized session snapshots and the client reassembles them per connection. The current v18 MMO projection policy still keeps at most 64 relevant units and packs as many 21-byte records as fit 1,077 bytes (46 without events, sheets, cooldowns or auras, 38 beside the largest self sheet); additional projected sections require a separate protocol and budget change.
- The Greyhaven inn (#158) is asset-tooling's `greyhaven-inn-v1` bundle, which `scripts/fetch-bundles.sh` downloads into ignored `web/public/buildings/` and `build.rs` embeds. `mmorpg_presentation::buildings` places its entries, swings the door over 0.4 s toward the replicated state and hides the roof and ceiling while the viewer stands inside; both clients run it. A click on the door leaf (picked by its mesh) sends `UseDoor` to flip it, and E or gamepad Start with no NPC selected uses the door in reach; the zone decides.
- asset-tooling's rolling releases (`humanoid-ranger-male`, `shrine-kit`, `outpost-kit`, `settlement-props`, `greyhaven-ambient-animals`, `combat-sounds`, `climbing-kit`, `crag-kit`) are read at whatever they currently hold, unpinned (#507): `scripts/fetch-bundles.sh` and `scripts/fetch-ranger.py` key each local copy on the live release's `export.ref.json`, keep it while that is unchanged (or the release is unreachable) and refresh it after an upload, so a new upload never needs a re-pin. Immutable versioned releases such as `greyhaven-inn-v1` are fetched once; `greyhaven-nature` is pinned by its tar's SHA-256 (#49). Tests check that the entries the game places exist and load, not asset hashes.
- The Sunken Shrine (#431) is asset-tooling's rolling `shrine-kit` release; `scripts/fetch-bundles.sh` unpacks it into ignored `web/public/scenery/` and `build.rs` embeds every module. `mmorpg_scenery::shrine::placements` lays the modules over the zone's shrine colliders and `mmorpg_presentation::shrine` places them once; both clients draw them, and the roof slabs stay procedural rock.
- The Outpost's smithy (collider 102) and house 105 (#420) are asset-tooling's rolling `outpost-kit` release, unpacked into ignored `web/public/buildings/outpost.kit`; `build.rs` embeds every module and roof variant. `content/greyhaven/dressing/outpost.json` stores all adopted module placements, and `mmorpg_presentation::outpost_kit` passes them through the shared world-dressing reader to the existing placed-kit path. The offline layout exporter composes the data-defined house/smithy templates; runtime clients never derive these transforms from collider footprints. Doors stay closed and the existing gameplay bodies stay solid. Their procedural blockouts are `Motion::Fallback` parts of the static scene: the browser draws them until the kit loads (and keeps them if it fails); the native client, which embeds the kit, never shows them.
  Each adopted building has its own cosmetic wear, saved in the dressing data (the offline exporter preserves the original per-building values). When the bundle ships the kit's wear masks (`outpost.kit.wear-masks`, asset-tooling docs/outpost-kit.md#wear-masks), the browser ages each building in the kit's lit shader (`web/src/world/outpost-wear.ts`): plaster stains and muted clay, greyed timber, and moss remain, but black pixel-only plaster holes and missing roof units are suppressed until geometry can support them. The wear rides in the batch's instance colour alpha, so every wear level draws in the pristine material's batch, and a bundle without masks draws pristine. The kit's KTX2 textures load through the shared `KTX2Loader` (transcoder in `web/public/basis`). The native client draws the pristine modules, and the authored fort with them, through the same library as the trees and rocks (`crate::nature`, one level each, #753) with their base-colour, normal and metallic-roughness textures (KTX2 transcoded by the `basisu` crate, as `crate::ground` does); baked into vertex colours, as before, their stone walls read flat grey. Picking still tests the modules' vertex-coloured draws (`bundle_glb` hands the `gltf` crate a PNG for each KTX2 image), which the frame carries as `occluders` without drawing them.
- The Outpost market square (#421) before house 106 is asset-tooling's rolling `settlement-props` release, unpacked into ignored `web/public/scenery/settlement.props`; `build.rs` embeds every prop. `content/greyhaven/dressing/market.json` stores the 23 committed prop transforms, including their surface heights, and `mmorpg_presentation::settlement_props` reads them through shared world dressing. The offline `export-market-dressing` example composes the data in `authoring/market-props.json` with the existing layout anchor; Blender export regenerates it when house 106 moves or is removed. The native client draws the props through the same library as the trees and rocks (`crate::nature`, #53), with their packaged base-colour, normal and metallic-roughness textures instead of baked vertex colours; the browser draws them through their glTF materials (`KitModules`). Core owns the existing solid market proxies (#577), independently of visual dressing; this migration leaves their geometry, paths, bench interaction and authoritative collision unchanged. Until the browser draws the bundle (or if it fails) the static scene shows a `Motion::Fallback` part of boxes from the descriptor's collision proxies; the native client, which embeds the props, never shows it.
- Greyhaven's rocks are drawn as blocks of asset-tooling's rolling `limestone-rocks` release, unpacked into ignored `web/public/scenery/limestone-rocks.kit`. `mmorpg_presentation::nature::LIMESTONE_ROCKS` picks a block for each proxy and fits it into the rock's collider box. Only the art changed: a rock still collides as its `mmorpg_core::ROCK_PROXIES` hull.
- A kit whose descriptor declares a layered surface (#732, asset-tooling#567; first `limestone-rocks --surface layered`) draws its untextured pieces through a blend in both clients: `mmorpg_presentation::surface` parses the descriptor's `surface` block, picks Greyhaven's weathering (`GREYHAVEN_WEATHERING`, medium) and owns the blend of the `_SURFACE_MASK` vertex attribute (R soil, G moss, B grass, A cavity) over the shared library (`<kit>.surface-library`: limestone colour and normal, moss, soil) at each layer's `tileMetres`. The browser patches the pieces' `LitMaterial` with that blend, triplanar in object space per fragment (`web/src/world/layered-surface.ts`, numbers through `mmorpg-wasm`'s `kitSurface`). render-engine has no custom materials, so the native client (`crate::nature`) draws each piece as box-projected, normal-mapped limestone with the cavity shade in its vertex colours, plus a soil and a moss overlay on the same triangles, alpha-blended by vertex alphas that composite to the blend's weights; the overlays cast no shadows and drop out while a level cross-fades. A baked kit draws as before. No layered release is fetched yet: to try one, unpack a locally built `limestone-rocks-layered.kit.tar` into `web/public/scenery/limestone-rocks.kit`.
- Greyhaven's brook, inflow and outlet are dressed from asset-tooling's rolling `river-stones` release, unpacked into ignored `web/public/scenery/river-stones.kit` and placed by `mmorpg_presentation::river` along the drawn water course. The village brook goes underground at a limestone sink (`river::sink`, collider `brook::SINK_ROCK`), one authored point before the physics lane ends at the old drain, which lies unseen behind the rock.
- Greyhaven's trees, rocks and bushes (#49, #596) are asset-tooling's rolling `greyhaven-nature` release, unpacked into ignored `web/public/scenery/greyhaven.nature`; `build.rs` embeds every entry's `default`, `lod1` and `lod2` and the `nature-descriptor`. `mmorpg_presentation::nature` places each scenery tree (oaks and birches the broadleaf, pines the conifer) each bush (a stable pick of the round, spreading and upright bush, scaled uniformly to the procedural bush's volume, walk-through foliage) and each rock that is not climbing rock (the shore rocks and boulders keep their procedural rock and worn handholds, #512) where the procedural model stood, and its `Lods` picks each placement's level from the descriptor's `lodSelection` (projected bounding-sphere diameter over viewport height, with 8 % hysteresis; the two trees take `nature::TREE_MIN_SCREEN_SIZES`, nearer than the descriptor's, #615) and cross-fades a switch over 0.35 s; while the camera rests and nothing fades it evaluates no placement and reports the result unchanged (#593). The native client imports the entries once (`crate::nature`: textures with mipmaps, normal maps, alpha masks) and uploads them once into render-engine's `World`, whose `prepare` frustum-culls the instances per view; a fading level draws a dissolve copy of its mesh (per-vertex alpha noise through a masked material), the browser a screen-door dither (`web/src/world/placed-kit.ts`) on separate instanced meshes, so settled trees and rocks draw through their plain glTF material and keep early depth testing. A loose rock collides as its bundle rock's `collisionProxy` convex hull (`mmorpg_core::RockShape`, transcribed in `mmorpg_core::ROCK_PROXIES` and checked against the embedded descriptor) fitted into its collider box, which stays the zone's footprint; physics-engine has no hulls, so the physics A/B comparison collides rocks as their boxes on both backends. Until the browser draws the bundle (or if it fails) the static scene shows the procedural trees, rocks and bushes (`Kit::Nature` fallback groups); the native client never does. `outpost_frame_time` measures the hub and the woods.
- Greyhaven's ladders, climbable ropes and climbing ivy (#621) are asset-tooling's rolling `climbing-kit`, unpacked into ignored `web/public/scenery/climbing.kit`. `build.rs` embeds every part's `default`, `lod1` and `lod2` and the `climbing-descriptor`. `mmorpg_presentation::climbing` places them: each ladder collider is a stack of `ladder-section`s under a `ladder-top` whose parapet top is the tower top. Each rope collider is a `rope-end`, `rope-section`s and a `rope-anchor` with its axis 0.3 m off the face. Ivy is `vine-3m` covers or `vine-strip`s under a `vine-top`, on the boundary climbing routes' steep terrain and on the faces in `climbing::COVERS`. The native client draws the parts through the same library and `Lods` as the trees and rocks (`crate::nature`). The browser draws them through `web/src/world/placed-kit.ts`, as it draws the nature bundle. Until the browser loads the kit it shows the procedural ladders and ropes (`Kit::Climbing` fallback groups) and no ivy. On a ladder, `units::imported::Playback` follows the rung grid (`climbing::rung_offset`) with the `Ladder_*` loops' phase, and a still climber is drawn up to 0.15 m higher or lower so the hold's hands meet the rungs.
- Greyhaven's highland crags (#668) wear asset-tooling's rolling `crag-kit`, unpacked into ignored `web/public/scenery/crag.kit`. `build.rs` embeds the thirteen face, corner and cap modules and the five outcrops with their `lod1` and `lod2` and the `crag-descriptor` (not the macro texture). `mmorpg_presentation::crag` lays each crag box out by the kit's layout rule (cells about 3 m square, scaled to fit, no variant beside or above its own, corner columns, caps and cap-corners; not mirrored, since a negative instance scale would turn the faces inside out): climbable modules on the crags the content marks rock, sheer ones on `SHEER_CRAGS`. The box stays the collider. Both clients draw the modules like the climbing kit, with the shared `Lods`; until the browser loads the kit it shows the plain rock boxes and their worn handholds (`Kit::Crag` fallback groups). The descriptor's `climb` metadata places climbers: on a dressed climbable face the drawn body stands out by the face plane plus the `Climb_*` clips' `standOff` less the body radius (`crag::Faces`, `UnitPresenter::set_crag_descriptor`), and the climbing kit's ivy on a dressed crag hangs `crag::IVY_STAND_OFF` out, clear of its relief. The Redbrand Hollow's cliffs are authored meshes, not boxes, and keep their own rock skin. The outcrops (#683) are placed by the world dressing data (`content/greyhaven/dressing/`, `mmorpg_presentation::world_dressing`) through the same placements and levels; while the kit is not drawn, their generated collision surface is the visible stand-in.
- Greyhaven's ambient cat, dog and rodent (#419) are asset-tooling's rolling `greyhaven-ambient-animals` release, unpacked into ignored `web/public/scenery/greyhaven.ambient-animals`. `mmorpg_presentation::ambient` walks each round a small authored loop (cat by the inn, dog before the Millbrook farmhouse, rodent by the barn crates) as a pure function of the presentation clock with `Idle`/`Walk` clips; both clients skin and draw them. They are cosmetic: no unit IDs, AI, collision, picking or replication; a bundle that fails to load leaves them absent.
- The built-in Greyhaven Vale (content revision 10, with creatures, NPCs, items and equipment, loot rules, creature abilities, a vendor, the quest chain and the walk-in inn) and snapshot v22 change the standalone host's initial simulation state. Recovery bundles captured with earlier content or snapshot versions cannot be silently reused; arrange an explicit migration or fresh development state.
- Sound (#447, Atlas 6.6.37) is the shared [game-audio](https://github.com/moritzbrantner/game-audio) director on its kira backend, playing asset-tooling's CC0 rolling `combat-sounds` release (asset-tooling#297), unpacked into ignored `web/public/sounds/combat-sounds` and embedded by `build.rs`. `mmorpg_presentation::audio` maps each received projection to cues, the same mapping Pages plays: hits (`combat.hit`, `combat.hit.defeated`), hurt, blocked, guard broken, the viewer's whiff, refused intents, the viewer's defeat, and a wind-up cue for Stomp, Redbrand Whirl and Overhead Smash. Only units in the projection sound; cue keys (zone/tick/event index) keep a reconnect or late projection silent; a wind-up fades on interrupt, death, leaving interest or an overdue resolution. The listener is the camera, audio suspends while the window is unfocused or minimised, and Settings → Audio (master, effects, interface volume, mute) reaches the mixer through the `settings` audio adapter. The wind-up uses the bundle's counter-open chime until a dedicated wind-up sound exists. Audible playback is not checked by tests.
- Linux is the exercised desktop platform in this change. Windows/macOS builds and installers remain unverified.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo build --workspace --all-features --locked
python3 scripts/smoke-native.py --window
```

Native smoke also paints the egui interface over a projected world (`the_egui_interface_paints_over_the_native_world`); `MMORPG_SMOKE_FRAME_DIR` saves it as `interface.ppm`.

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

Native world selection queries the last drawn model parts and the uploaded terrain, water and prop geometry through pinned `3d-lab::three_d_projective::picking`. The client retains EntityRef ownership; health bars and target markers are not selectable bodies. As in the browser (#369), selection is forgiving (#370): an exact hit always wins, but a click that hits nothing selectable may still select a creature, NPC or the inn's door leaf when it lands within 8 logical pixels of that target's projected bounds and the target stands in front of whatever the ray did hit, so walls, roofs, terrain and other players still block. Among several near misses the smallest miss wins, then the target nearer the eye. These proxy boxes are presentation-only; colliders, zone selection and wire formats are unchanged. NPC context uses existing content and received quest/bag sheets, advertises navigation in the window title, and sends existing quest/vendor commands only after a deliberate action. The zone validates state, reach and settlement; refusals remain visible in the context. Focus/leave/cancel retires armed clicks through the shared input runtime; reconnect retires one-shot interactions along with the existing combat presses.

## Durable development characters

Set `MMORPG_CHARACTER_DIR` on the zone host to enable development/LAN character
storage, for example:

```sh
MMORPG_CHARACTER_DIR=target/dev-characters ./scripts/dev-native.sh
```

The selected roster character claims its server record before gameplay starts.
Its stable online identity is kept in `online-character-ids.json` beside the native
roster; the file contains IDs only. A roster slot with an online identity is never
reissued after deletion. Normal window exit commits/releases the character; network
interruption uses the existing reconnect path. Anonymous hosts remain compatible.

With `--class`, the native client uses a stable per-profile identity for that
class/sex. `--character-id ID --class ranger` explicitly names a development record
instead, for example to resume the same character from another trusted device.
Claims are unauthenticated; this mode is for a single development/LAN host, not a
public deployment. [Ownership and recovery](adr/0003-development-character-admission.md)
remain server responsibilities.

## PostgreSQL journals and S3 checkpoints

Build the host with `cargo run -p mmorpg-game-server --features postgres --locked --bin mmorpg-zone-host`.
`MMORPG_PERSISTENCE_MODE` selects the storage composition; setting it on a build without that feature fails explicitly.
It cannot be combined with `MMORPG_CHARACTER_DIR` or `MMORPG_RECOVERY_DIR`.
Both clients keep their existing character-claim protocol. Identity remains the existing unauthenticated development/LAN contract; this does not select a public authentication service or deploy a fleet.

Configure distinct login roles granted the migration-created roles below, on one PostgreSQL deployment with a physical standby:

| Environment variable | Connection / role |
| --- | --- |
| `MMORPG_COORDINATOR_DATABASE_URL` | Primary, `mmorpg_zone_coordinator` |
| `MMORPG_COORDINATOR_STANDBY_URL` | Designated physical standby, coordinator role |
| `MMORPG_CHARACTER_STANDBY_URL` | Same standby, read-only `mmorpg_character_profiles` |
| `MMORPG_ZONE_DATABASE_URL` | Primary, `mmorpg_zone_journal` |
| `MMORPG_ZONE_STANDBY_URL` | Same standby, zone-journal role |
| `MMORPG_HOST_ID` | Host/process identity; reacquisition always advances its zone epoch |
| `MMORPG_CHECKPOINT_S3_ENDPOINT`, `MMORPG_CHECKPOINT_S3_BUCKET` | S3-compatible checkpoint destination; provision the bucket first |

Credentials follow the S3 SDK's provider chain, including `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY`.
`MMORPG_CHECKPOINT_S3_REGION` defaults to `us-east-1`; local HTTP needs explicit `MMORPG_CHECKPOINT_ALLOW_HTTP=1`.
The initial tested provider is RustFS `1.0.3@sha256:9ca9559533b537d10482fe923288b676ad08f92a7069d9a17327fbae3cab688b`.

Use `MMORPG_PERSISTENCE_MODE=migrate` with `MMORPG_MIGRATION_DATABASE_URL` pointing to a privileged migration connection before starting the host. Normal startup never changes schemas.
Then use `bootstrap` for a new stream, and `recover` for subsequent starts. Bootstrap refuses an existing checkpoint; recovery never silently creates or resets a world.
Graceful termination checkpoints committed state, stops/join storage workers, then retires ownership. A crashed host needs explicit `retire` after its process is stopped: configure one zone, `MMORPG_EXPECTED_ZONE_EPOCH`, and `MMORPG_CONFIRM_SOURCE_STOPPED=1`.
`reset` also requires that confirmation and already-retired assignments; it publishes a verified empty-world checkpoint in a new timeline while preserving character records, world identity and grant receipts.
`profile` with `MMORPG_PROFILE_CHARACTER` prints committed character progression from the standby without starting or locating a zone host.

Operational limits are configurable, not performance ratchets:

| Setting | Default |
| --- | ---: |
| `MMORPG_GRANT_TTL_MS` / `MMORPG_GRANT_RENEW_MS` | 30000 / 10000 |
| `MMORPG_PERSISTENCE_TIMEOUT_MS` / `MMORPG_CHECKPOINT_TIMEOUT_MS` / `MMORPG_COMMIT_TIMEOUT_MS` | 5000 / 5000 / 25000 |
| `MMORPG_CHECKPOINT_INTERVAL_TICKS` | 300 |
| `MMORPG_JOURNAL_MAX_BATCH_BYTES` / `MMORPG_JOURNAL_MAX_RETAINED_BYTES` | 16777216 / 67108864 |
| `MMORPG_JOURNAL_MAX_BATCHES` / `MMORPG_JOURNAL_MAX_REPLAY_TICKS` | 900 / 900 |
| `MMORPG_CHECKPOINT_MAX_COMPRESSED_BYTES` / `MMORPG_CHECKPOINT_MAX_DECODED_BYTES` | 16777216 / 67108864 |
| `MMORPG_MAX_REWARD_RECEIPTS` / `MMORPG_MAX_TICK_GRANTS` | 1024 / 128 |

Lease/commit failures fence the host. Object-store failures retain the last verified checkpoint and bounded PostgreSQL journal; exceeding its configured bounds stops progression.
Acknowledged economy depends on the designated standby's durable receipt and replay. Promotion must preserve that confirmed history and fence the former primary; automatic database failover is not implemented here.
The focused `persistence_host` test exercises a real WebTransport claim, complete host-process loss, journal recovery, reconnect and durable logout against PostgreSQL plus RustFS, without a GPU.

## Trusted-LAN handoff hosts (#145)

Hosts of neighbouring logical zones hand characters to each other through seams and waystones when each runs the `postgres` build with `MMORPG_HANDOFF_DATABASE_URL`, one PostgreSQL database they share ([ADR 0007](adr/0007-seam-corridors-and-character-transfer.md#shared-control-plane-and-live-hosts-part-3b)). The host migrates the character and coordination schemas at start. It cannot be combined with `MMORPG_CHARACTER_DIR` or `MMORPG_RECOVERY_DIR`. Claims and the transfer link are unauthenticated: trusted LAN only.

| Environment variable | Meaning | Default |
| --- | --- | --- |
| `MMORPG_HOST_ID` | This host's identity; a restart under the same ID takes back its own zone grants | required |
| `MMORPG_TRANSFER_LINK` | This host's transfer link address | `127.0.0.1:4500` |
| `MMORPG_ZONE_ROUTES` | `zone=host:port@session-route,…`: each peer zone's transfer link and the route its clients connect to | none |
| `MMORPG_GRANT_TTL_MS` | Zone grant lifetime; renewed every third | 30000 |
| `MMORPG_HANDOFF_TIMEOUT_MS` | Database statements and transfer link requests | 2000 |
| `MMORPG_DEPTHS_GRACE_MS` | How long an emptied Redbrand Depths run stays resumable (#404); add depths slot IDs (20000–20031) to `MMORPG_ZONE_IDS` and `MMORPG_ZONE_ROUTES` to host runs | 900000 |

For example, Seam Test West and East on one machine (certificates as for any host):

```sh
MMORPG_HANDOFF_DATABASE_URL=postgres://… MMORPG_HOST_ID=west MMORPG_ZONE_IDS=901 MMORPG_PORT=4433 \
  MMORPG_STATUS_PORT=9101 MMORPG_TRANSFER_LINK=127.0.0.1:4501 \
  MMORPG_ZONE_ROUTES=902=127.0.0.1:4502@https://localhost:4434/game/matches/zone-902 \
  cargo run -p mmorpg-game-server --features postgres --locked --bin mmorpg-zone-host
MMORPG_HANDOFF_DATABASE_URL=postgres://… MMORPG_HOST_ID=east MMORPG_ZONE_IDS=902 MMORPG_PORT=4434 \
  MMORPG_STATUS_PORT=9102 MMORPG_TRANSFER_LINK=127.0.0.1:4502 \
  MMORPG_ZONE_ROUTES=901=127.0.0.1:4501@https://localhost:4433/game/matches/zone-901 \
  cargo run -p mmorpg-game-server --features postgres --locked --bin mmorpg-zone-host
```

A departing session receives a redirect notice ([PROTOCOL.md](PROTOCOL.md#redirect-notice-145)). Both clients follow it: they connect to the destination's route, claim the same character there and draw the destination zone's scenery; the source's route and resume token are never used again ([ADR 0007](adr/0007-seam-corridors-and-character-transfer.md#client-cutover-part-3c)). Start the native client in a Seam Test zone with `--zone 901 --url https://localhost:4433/game/matches/zone-901`. `python3 scripts/smoke-handoff.py` runs the whole journey: PostgreSQL 17 in Docker, both hosts, and `mmorpg-client --autopilot` walking West → East through the seam and teleporting back by waystone. The focused `handoff_hosts` test runs both host processes against `MMORPG_CHARACTERS_POSTGRES_URL`: a walker crosses the seam and a traveller teleports by waystone, each continuing at the other host.

### Client Settings and character controls

The **Settings** button (beside Enter World on character selection, bottom right in
the world) opens a modal on character selection
and in the world. Field of view, look sensitivity, vertical look inversion and
reduced motion use the same versioned catalog as the browser. Changes apply
immediately and save local delta-only User/Device snapshots; save failures remain
visible. Settings blocks local gameplay input while the online zone continues.

Select a roster character to edit its Controls. Keyboard recording (including
modifiers), mouse buttons and controller buttons/stick directions share one
`input-bindings` profile for that stable character identity. A new conflict must
receive an explicit offered repair before it changes saved/runtime controls.
Closing Settings or losing focus cancels unconfirmed recording. Profiles are
local files beneath the configuration directory's `bindings/`, separate from
ordinary Settings snapshots and authoritative character progression.

A focused offscreen view uses the existing native renderer test:

```sh
MMORPG_NATIVE_SETTINGS_EVIDENCE=1 MMORPG_SMOKE_FRAME_DIR=target/settings-frames \
cargo test -p mmorpg-client --locked --lib the_egui_interface_paints_over_the_native_world -- --ignored --nocapture
```

It emits `settings.ppm`; this demonstrates rendering, not OS focus or hardware
input. Settings uses `content/settings/catalog.json` for platform applicability,
category order, translated labels and canonical presentation metadata. Global
search finds preferences across categories; category/global reset requires
confirmation and preserves dormant imported entries. The current catalog exposes
only real Immediate preferences; it adds no speculative restart/reconnect or
renderer-quality controls.

Expand **User preferences file** to export to a filesystem path or preview an
import. **Replace User preferences** replaces only User overrides after a
successful atomic save; Device preferences and character bindings remain intact.
The browser exposes the same schema-v2 file through download/upload. Both clients
retain same-scope entries for other platforms without applying or exposing them.

Native snapshot files are `settings.user.json` and `settings.device.json` beneath
the local configuration directory. Bindings remain separate portable
`input-bindings` profiles keyed by the stable roster identity. There is no cloud
sync or server preference authority.

Terrain tiles (#526, `mmorpg_presentation::tiles`): the terrain grid is cut into 128 m tiles (64 m tiles drew twice the draws). A tile draws its near level (the textured ground layers of #525 over its cells, built on demand) within 160 m of the camera once resident, else its far level (the relief every 8 m, with gentle ground, roads, the plaza and fields baked into vertex colours while steep faces retain rock colour/roughness detail without the near normal/occlusion maps); far levels draw merged into the largest 512 m or 256 m chunk that holds no near tile. Skirts hang below every tile edge, as deep as the coarse edge strays from the fine one, so a near tile beside a far one shows no crack. The shared `Streamer` builds near levels within 208 m, nearest first and two per frame (every wanted one on the first frame), and evicts them beyond 256 m; tiles holding a terrain hole always draw near. The native client also streams the static scenery groups: a group is lowered when first shown and dropped 64 m past its cull distance, so the renderer frees its meshes. The browser draws each ground material as one `BatchedMesh` (`web/src/world/ground.ts`) that draws all its shown tiles in every view: culling it per view re-uploaded its draw list in each sun cascade and stalled the GPU. The browser streams the static groups (#526) through the same `Streamer` over `Group::site`: a group is built within 40 m of its class's reach (eight per frame, nearest first; groups within 72 m on the first frame) and dropped 64 m past it, and a drawn kit's fallback is dropped; `WorldView.load` no longer builds any of this, the idle time before entry or the first frame does, so entry waits only for what surrounds the character. The terrain grids, the slowest derived data, are kept per WASM release in Cache Storage (`web/src/asset-cache/derived.ts`; a cold or cleared cache derives them again). The distant ranges are shaded by their relief: a baked hillshade, lighter ridges and darker gullies, rock strata, grass on gentle low slopes and snow by altitude where it is not too steep, on softened facets.

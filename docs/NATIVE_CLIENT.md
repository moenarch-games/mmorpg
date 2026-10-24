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
| X | Swim down while held (#274); held Space swims up |
| Tab | Select the nearest living attackable creature; press again to cycle outward |
| F | Start auto-attacking the target, or stop |
| R | Release your spirit while dead |
| 1–8 | Use action-bar slot 1–8 (the class kit in catalog order) |
| Left click | Select that visible creature or NPC, without attacking |
| N | Cycle visible NPCs |
| Enter | Open the selected NPC’s context (quest/vendor window); with no NPC selected, focus chat |
| B / C / L | Toggle bags / character / quest log |
| Chat field | Enter sends (`/s`, `/y`, `/wave`, `/bow`, `/cheer`, `/laugh`, `/point`), Escape returns to the world |
| Interface clicks | Action-bar slots, Attack/Talk/Loot/Clear on the target frame, quest Accept/Complete with reward choice, Buy/Sell, bag move/split/merge/Equip, Unequip, Take all, Abandon, Release spirit |
| Context: Up / Down | Browse every quest, stock offer and bag slot |
| Context: 1 / 2 | Accept/complete the displayed quest or buy one displayed item / sell one from the displayed bag slot |
| Context: Left / Right | Choose a quest reward |
| Quest context: H | Page through the authored dialogue and named objectives |
| Left or right mouse drag | Orbit the camera around your character (five-pixel click threshold) |
| Mouse wheel | Zoom the camera |
| Escape | Close person context or cancel an armed click; otherwise cancel a cast/channel, close open panels, or quit |

The third-person camera follows your chosen class/sex appearance. Its first view chooses an unobstructed heading from the same ordered candidates as the browser; subsequent projections preserve user orbit. The eye stays above the rendered terrain under it. While a movement key is held, your character turns to face the camera's direction, so W always runs away from the camera; releasing the keys leaves the character facing where it last moved. Running and strafing move at 6.3 m/s, backpedalling is slower. Losing focus clears held movement, and so does dying: while dead the client sends no held movement or jumps, and held keys take effect again once a projection shows you alive. Movement is sent on change and retransmitted at 20 Hz so a lost key-release datagram is corrected; each Space, Tab, F, R, 1–4 or cast-cancelling Escape press sends one command and is never replayed after a reconnect. Without `--class`, the window opens on the local character roster and sends nothing until you enter the world: pick a character or create one (name, class, body and headwear, with the browser's rules: 2–24 letters, unique names, at most 8 slots, lowest free `local-N` ID), change its headwear or delete it, and turn your avatar on a turntable preview (±15° steps, slider, Front) before entering. The roster is local presentation choice only, kept as versioned JSON in `roster.json` under `$MMORPG_CONFIG_DIR` or the platform configuration directory (`$XDG_CONFIG_HOME`/`~/.config/mmorpg`, `~/Library/Application Support/mmorpg`, `%APPDATA%\mmorpg`); an invalid file fails closed and is never overwritten, and no world progress is stored. Entering sends the existing `ChooseClass`; `--class warden|ranger|arcanist` and `--sex female|male` (male by default) skip it; the session sends `ChooseClass` with every heartbeat, under fresh sequences, until a projection shows a class (so a lost datagram cannot leave the character classless, also across reconnects), keys 1–8 use the class kit at the current selection, and Escape cancels a running cast instead of closing the window. Keys 1–8 and the cast-cancelling Escape resolve through the shared `input-bindings-core` runtime (`controls.rs`): the same semantic actions, contexts and default keys as the browser (`ability.slot1`–`8` in `gameplay`, `combat.cancelCast` in a non-blocking `casting` layer pushed while the latest projection shows a cast), one use per press, no OS key repeat, and focus loss releases every held action. Holding Escape never closes the window; only a fresh press with no context, armed pointer or cast does. The interface (`ui.rs`, #183) composes external [egui](https://crates.io/crates/egui) 0.37 through `egui-winit` and `egui-wgpu`, painted into the resolved frame after the world. It shows unit and target frames (health, class resource, XP, copper, auras, target cast and target-of-target), the cast bar and clickable action bar with cooldown/global-cooldown shading, a refusal banner, chat and combat logs, a rotating minimap with roads, water, buildings, unit dots and quest markers, the quest tracker and log, NPC quest dialogue and vendor windows, bags, the character pane, corpse loot and the dead state. Behind the windows, world-space nameplates (disposition colour, larger for the target) and NPC quest `!`/`?` markers hang above each presented unit's posed model head, and floating combat text (damage, criticals, periodic ticks, damage taken, heals, Absorb, Miss/Evade/Whiff, interrupts — the browser's `combat-text.ts` rules) rises over the unit each projection event concerns. Everything is read from the received projection, the retained self sheets and immutable content; each control sends an existing shared command and the zone validates it. egui receives pointer events always but the keyboard only while the chat field is focused: a press on a panel never reaches the world (releases always do), focusing chat retires held movement and actions, and focused buttons never swallow movement keys. B, C, L and Enter resolve through the same `input-bindings-core` catalog as the browser (`ui.toggleBags`, `ui.toggleCharacter`, `ui.toggleQuestLog`, `ui.openChat`). Enter keeps the native selected-NPC context contract and opens chat otherwise. The title still shows status and context text.

A standard gamepad plays the window like Pages (`gamepad.rs`). Devices come from external [gilrs](https://crates.io/crates/gilrs) 0.11, through the `gilrs-azul` 0.11.3 fork, whose only differences are loading libudev at run time (no libudev development headers needed to build; without libudev there are simply no pads) and an unused serial accessor. Pads are sampled once per frame in W3C standard mapping and resolve through the same `input-bindings-core` runtime with the browser's pad catalog (`web/src/input/game-controls.ts`: same action ids, button/axis indices, 50 % threshold and 20 % axis deadzone hysteresis): the left stick runs and strafes and feeds the same held-movement path as W/A/S/D (the camera-facing rule included), A jumps, X toggles auto-attack, Y releases the spirit, RB targets the next unit, the d-pad uses slots 1–4 (up, right, down, left), Select toggles bags and B cancels a cast. Natively the right stick orbits the camera at an analog rate, LB cycles NPCs, Start talks to the selected NPC, L3 toggles the character pane and RT the quest log. Holding LT raises the Warden's shield guard (#156): the right stick's vertical picks the lane (up high, centred middle, down low) and the camera stops tilting meanwhile; release lowers it, as in Pages. While a window is open (bags, character, quest log, loot, an NPC context or character selection), a non-blocking `panels` layer gives B "close", the d-pad Shift+Tab/Tab focus movement and A Enter on the focused egui control; movement and other gameplay buttons still fall through. B never leaves the world: the native window has no character screen to return to. Window focus loss, a pad disconnect and every context change retire held pad actions (a still-deflected stick resumes, a still-held button does not fire twice), and pads are ignored while unfocused or typing.

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
- Players are asset-tooling's Ranger humanoid (Atlas 6.3.7) and wolves, vermin and boars Quaternius CC0 animals (`assets/quaternius/*.glb`, Atlas 6.3.6); humanoid creatures and NPCs keep procedural models natively (Pages draws the named Greyhaven residents in their asset-tooling appearance presets, #418; the native client does not load those presets yet). `scripts/fetch-bundles.sh` downloads the Ranger bundle once into ignored `web/public/characters/`; `scripts/prepare-ranger-native.sh` prepares standard GLBs with asset-tooling; the client's `build.rs` embeds them and the shared animation library and fails with that hint while it is missing. Models are imported once with the `gltf` crate (prepared standard GLBs; shared animation targets bind by joint name), base colours and base-colour texels become vertex colours, and `presentation/skinned.rs` samples idle/walk/run/attack/death clips and skins on the CPU into ordinary draws, so picking sees the posed mesh. Every class and sex shares the Ranger until more character bundles exist; a Warden also draws the bundle's sword and shield, a Rogue the blackened dagger and the Necromancer, Cleric and Druid their bone, gilded and gnarled staffs (asset-tooling #375; `hand_r.staff`) at the `humanoid-sockets` hand sockets (placement × joint × socket each frame), and a standing Warden holding guard plays `Idle_Shield_Loop` (no layered upper-body blend: walking keeps the walk clip, and only the viewer's own guard is projected); models are cosmetic and never change bodies or hits. Pages presents the same models with plain three.js (`GLTFLoader`, `AnimationMixer`, socket parenting; `web/src/world/units/quaternius-units.ts`), where a Ranger also carries the bundle's bow and quiver on its back (native does not draw those yet).
- World/model/environment parity acceptance is documented in [NATIVE_WORLD_PARITY.md](acceptance/NATIVE_WORLD_PARITY.md); paired rendered review and native application checks are required before release. Native world clicking, selected-person quest/vendor windows, unit/target frames, action bar, bags, character pane, loot, chat, quest tracker/log and minimap are implemented with egui (#183); the keyboard context and title text remain. The local character roster with appearance persistence and turntable preview, floating combat text and world-space nameplates/quest markers are implemented (#183). Gamepad access is implemented (#183). The Settings menu and character-scoped rebinding are implemented; see [client Settings](#client-settings-and-character-controls).
- The shared transport fragments oversized session snapshots and the client reassembles them per connection. The current v18 MMO projection policy still keeps at most 64 relevant units and packs as many 21-byte records as fit 1,077 bytes (46 without events, sheets, cooldowns or auras, 38 beside the largest self sheet); additional projected sections require a separate protocol and budget change.
- The Greyhaven inn (#158) is asset-tooling's `greyhaven-inn-v1` bundle, which `scripts/fetch-bundles.sh` downloads into ignored `web/public/buildings/` and `build.rs` embeds. `mmorpg_presentation::buildings` places its entries, swings the door over 0.4 s toward the replicated state and hides the roof and ceiling while the viewer stands inside; both clients run it. A click on the door leaf (picked by its mesh) sends `UseDoor` to flip it, and Enter or gamepad Start with no NPC selected uses the door in reach; the zone decides.
- The Sunken Shrine (#431) is asset-tooling's rolling `shrine-kit` release, pinned by tar sha256 and `web/assets/shrine-kit/export.ref.json`; `scripts/fetch-bundles.sh` unpacks it into ignored `web/public/scenery/` and `build.rs` embeds every module. `mmorpg_scenery::shrine::placements` lays the modules over the zone's shrine colliders and `mmorpg_presentation::shrine` places them once; both clients draw them, and the roof slabs stay procedural rock.
- The Outpost's smithy (collider 102) and house 105 (#420) are asset-tooling's rolling `outpost-kit` release, pinned by tar sha256 and `web/assets/outpost-kit/export.ref.json` and unpacked into ignored `web/public/buildings/outpost.kit`; `build.rs` embeds every module and roof variant. `mmorpg_scenery::outpost_kit` transcribes the kit descriptor's `house` and `smithy` compositions onto the unchanged collider footprints (doors closed, colliders solid) and `mmorpg_presentation::outpost_kit` places them once. Their procedural blockouts are `Motion::Fallback` parts of the static scene: the browser draws them until the kit loads (and keeps them if it fails); the native client, which embeds the kit, never shows them.
- The Outpost market square (#421) before house 106 is asset-tooling's rolling `settlement-props` release, pinned by tar sha256 and `web/assets/settlement-props/export.ref.json` and unpacked into ignored `web/public/scenery/settlement.props`; `build.rs` embeds every prop. `mmorpg_scenery::settlement_props` transcribes the descriptor's 12 × 9 m `square` composition (stalls, handcart, crates, barrels, lantern post, bench, signpost, fence, planter, firewood) and `mmorpg_presentation::settlement_props` places it once; repeated props share one imported model. The props are presentation only: no colliders, picking or interaction, so paths and the authoritative colliders are unchanged. Until the browser draws the bundle (or if it fails) the static scene shows a `Motion::Fallback` part of boxes from the descriptor's collision proxies; the native client, which embeds the props, never shows it.
- Greyhaven's ambient cat, dog and rodent (#419) are asset-tooling's rolling `greyhaven-ambient-animals` release, pinned the same way by `web/assets/greyhaven-ambient-animals/export.ref.json`. `mmorpg_presentation::ambient` walks each round a small authored loop (cat by the inn, dog before the Millbrook farmhouse, rodent by the barn crates) as a pure function of the presentation clock with `Idle`/`Walk` clips; both clients skin and draw them. They are cosmetic: no unit IDs, AI, collision, picking or replication; a bundle that fails to load leaves them absent.
- The built-in Greyhaven Vale (content revision 10, with creatures, NPCs, items and equipment, loot rules, creature abilities, a vendor, the quest chain and the walk-in inn) and snapshot v22 change the standalone host's initial simulation state. Recovery bundles captured with earlier content or snapshot versions cannot be silently reused; arrange an explicit migration or fresh development state.
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

### Client Settings and character controls

The **Settings** button at the bottom right opens a modal on character selection
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

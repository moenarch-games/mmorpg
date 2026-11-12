# Battle Royale in the MMORPG browser client

## Practice arena (Pages, no server)

`?activity=battle-royale` (or the **Battle Royale** tab on the selection screen) opens the
**Loadout** screen ([CHARACTER_SELECTION.md](CHARACTER_SELECTION.md#one-front-end-in-both-clients)):
a class tile grid on the left, the build on the stage, equipment per slot and a **Look** on the
right (a roster character's look and a body; nothing on the roster changes), and the arena
route with **Practice** or **Hosted match** in the bottom bar. Practice is chosen unless the URL
names an `arenaServer`. Pick a class and equipment, then **Start practice**: the page's `mmorpg-wasm` host
(`LocalZone.forPracticeArena`) admits that `EphemeralCharacterBuild` at the maximum level just
inside the fortress's south gate, facing the keep across the courtyard and five stationary
**Training Targets** (three about 8–9 m ahead, two at 21 m). Targets are breakable props: they take real `mmorpg-core` damage, never move, fight or
hold threat, break, and stand again after 30 s. Directed strikes are **timed swings** that wind
up visibly, sweep over their active ticks and can clash ([PVP_COMBAT.md](PVP_COMBAT.md#timed-swings-and-clashes-679-arena-first)). There is no storm, matchmaking, team, reward
or other player. **Reset practice (F9)** starts a fresh life with the build chosen at entry;
**Change build** returns to setup to change it. On a phone the touch overlay shows **Target** and
**Attack** from the first moment, and **Menu** offers **Reset practice** and **Change build**;
in portrait the status line sits under the frames and the hotbar above the thumb controls
(#551). Nothing is saved, and the Greyhaven roster,
saves and the hosted path below are untouched.

### Netcode trial (#680)

Under **Practice** the route also offers a **Netcode trial**: Off (the practice above),
**1 · Baseline**, **2 · Predicted movement** or **3 · Predicted + hit rewind**
([PVP_COMBAT.md](PVP_COMBAT.md#arena-netcode-trial-680-atlas-59i680)), and a **simulated round
trip** of none, 50, 100 or 150 ms (split into whole uplink and downlink ticks, no jitter). The
URL can preset both: `?activity=battle-royale&netcode=rewind&delay=150`. A trial match
(`LocalZone.forArenaTrial`) lets players fight and adds a **sparring partner**, a standard Warden
the page drives inside the zone without delay: it closes to 2 m, circles and swings every 2.5 s
within reach, so movement, hits and clashes can be felt. The page's zone stays the one
authority; the trial source only delays commands to it and projections from it, sends timing-
sensitive commands with the tick it had observed, and draws other units 67 ms behind like the
online client. The option and delay hold for the match; **Reset practice (F9)** starts another
with the current choice.

In a **Hosted match** the same select chooses this client's movement: **1 · Baseline** (or Off)
interpolates the own body like every other unit; **2** and **3** predict it
(`ArenaMovementPrediction.forHostedArena()`, the fortress's own movement rules), reconciled
against the match's `acknowledged_sequence`. Latency is real, so there is no simulated round
trip. Hit rewind is the **match's** option, the same for everyone in it: the Battle Royale host
runs it for match ids listed as `<id>:rewind` (`BATTLE_ROYALE_MATCH_IDS=1,2:rewind`), which
changes the match's content revision. The page accepts either fortress revision and shows both
options in the arena status line (`· predicted movement · hit rewind`).

## The fortress (#552)

Practice and hosted matches show the **same symmetric multilevel fortress** that Battle Royale
hosts ([battle-royale #31](https://github.com/moenarch-games/battle-royale/issues/31),
`docs/ARENA.md` there). `battle-royale-core` (pinned in the root `Cargo.toml`, with a `[patch]`
pointing its `mmorpg-core` at this workspace) is its one definition; nothing here copies the
geometry.

- **Physics**: the practice zone is the fortress's own colliders, ramps and eight lifts, unchanged,
  in `mmorpg-core` and Rapier, with its own spawn lattice in the south courtyard (slot 0 at
  (0, −76) m, 16 × 32 slots 2 m apart) and the training targets. A hosted match runs
  `arena::content()` itself; the client refuses projections whose content revision is not
  `ArenaPresentation.contentRevision()`, i.e. that fortress's.
- **Rendering** (#591): asset-tooling's rolling `fortress-kit` bundle
  (`scripts/fetch-bundles.sh` → `web/public/scenery/fortress.kit`) draws the fortress in textured
  CC0 stone and timber. Its descriptor (`fortress-descriptor` on `fortress.kit.curtain-wall`) lists
  each piece's `origin`, `yawDegrees` and the arena bodies it dresses;
  `mmorpg_presentation::composition` keeps a piece only when every one of its bodies equals a body
  of `arena::content()` exactly (collider id, centre and half extents; ramp body, box and rise;
  lift platform body, low stop and half extents). Statics are placed once; the lift cribs follow
  `Platform::centre_at(tick)` minus their half height through the unit presenter, which then
  draws no pillar for them. Like Greyhaven's nature, each entry level is one `InstancedMesh` per
  primitive (`web/src/world/instanced-levels.ts`, #599), and the shared `mmorpg_presentation::lod::Lods`
  picks each piece's `default`/`lod1`/`lod2` at the descriptor's `lodSelection` screen sizes,
  cross-fades switches (only fading instances draw dithered), re-evaluates only when the camera
  or a lift moves, and culls pieces wholly outside the view by the descriptor's `bounds` box
  (pass 2's decoration included), not their collider. The terrain leaves out the four courtyard lift footprints, whose platforms rest flush
  with the ground. `Scenery::graybox` (every collider box a graybox prop, every ramp a wedge) stays
  the fallback per body: drawn while the bundle loads, if it fails, and for any body no piece fits.
  The ground is the ordinary flat terrain. The native client has no arena, so it draws neither.
- **Camera**: in the arena only, the orbit camera's eye is pulled in front of the first structure
  box between it and the character (`clearSightLine`), so it stays under the keep's roof and in
  front of walls; Greyhaven's camera is unchanged.

The art is presentation only: each piece is fitted to its bodies (pass 2's decoration may stand
slightly proud of them), and collision is still `battle_royale_core::arena`'s alone. A later pass
of the kit with the same descriptor format is adopted by refetching the rolling release.

`scripts/smoke-browser.py` walks courtyard → keep door → corridor → interior ramp → roof → central
tower lift → tower top through the page's real zone at desktop and phone size
(`test_battle_royale_fortress_walkthrough_*`), and `mmorpg-wasm`'s tests walk the same zone onto
the wall walk, a corner tower and a courtyard lift. `scripts/profile-browser.py --activity
battle-royale` profiles the arena.

![Practice character on the keep roof, with lifts, bridges and the curtain wall](reviews/battle-royale/fortress-roof.png)

## Hosted match

Choose **Hosted match** in the bottom bar. Select a class, a look and equipped
items, then join the same **Match server** as a friend. Characters enter at the
shared supported maximum level, with actual class resources and abilities. Equipment
choices are filtered by the shared item catalog's slots; the host validates them.
This catalog currently has no class or item-level restrictions, and the available
armor is the same for all three classes. Defaults come from `EphemeralCharacterBuild`.

The client reuses its ordinary Three.js renderer, Rust unit presenter, class HUD,
weapons/spells, targeting, shield guard, movement, input bindings and Settings.
Tab and pointer selection include admitted attackable arena players; ordinary safe
players remain excluded. The fortress (above) uses the same Rust static-scene machinery,
with no Greyhaven props or relief and no second animation/rendering system.

Setup is frozen on joining, and the host freezes the life at Drop. During the match,
class/equipment edits and PvE resurrection are unavailable. Health, resources,
opponent casts/impacts and death come from the actual shared zone projection. The
mode status shows remaining players, elimination and winner. A confirmed final
projection closes the completed session while retaining its result; late disconnects
cannot replace the result with a reconnect error.

After elimination or the result, **Change build / next match** returns to setup and
suggests the next numbered match route. The host must already serve that match; this
is not automatic matchmaking or match creation. Edit the route when joining another
configured match. Class/gear changes then apply to the next admitted life.

Match setup stays in page memory. The activity never claims the host's durable
character service and never joins or saves the local Greyhaven character/world.
Appearance/roster, account persistence, rewards, rankings, public matchmaking and a
public fleet are not added. GitHub Pages serves the client; it does not operate an
authoritative Battle Royale server.

## Local hosted path

Use the `battle-royale` revision this workspace pins,
**91700ebca3d5f231a3b043383de8a7c9f7675396** (battle-royale #50, the arena's timed swings and
clashes, #679), or a later one whose core/protocol pin carries this checkout's `PROTOCOL_ID`
(it pins MMORPG `ed38765`). Mode protocol 2, schema 2 and ruleset 3 wrap the MMORPG snapshot
and command. Character commands echo the sheet revisions the session's decoder holds, and the
host sends a sheet only until a command acknowledges it, as a zone host does. Ruleset 3's storm
waits 60 s and closes over 120 s, so it reaches no spawn slot in the first two minutes of
combat. Different versions or another arena content revision fail closed.

With that checkout beside this one:

```sh
BATTLE_ROYALE_ROOT=../battle-royale ./scripts/dev-browser-battle-royale.sh
```

The launcher reuses the ordinary short-lived development TLS helper, builds the mode
host and starts Vite. Open its printed URL in two tabs. It preconfigures rounds 1–4;
`BATTLE_ROYALE_MATCH_IDS` may select another set. Ctrl-C stops both processes.
`MMORPG_DEV_TRANSPORT_PORT`, `MMORPG_DEV_STATUS_PORT` and `MMORPG_DEV_WEB_PORT` override
4433/8080/5173. For another server, fill Match server and, only for a self-signed
development certificate, its certificate hash. A publicly trusted host needs no hash.

The URL can preselect the activity and address:
`?activity=battle-royale&arenaServer=https://host:4433/game/matches/br-0000000000000001`.
`certHash` has the same SHA-256 format as ordinary online play. Reconnect tokens remain
in memory and never enter the URL. The native client has no activity entrance in this
slice; this is the browser entrance using the shared native/browser presentation owner.

## Focused verification

Build `web/dist`, build the pinned mode host, then run:

```sh
python3 scripts/smoke-browser-battle-royale.py --host /path/to/battle-royale-host
```

This explicit smoke reuses the ordinary online smoke's TLS, ports and static-file
handler. Two actual Chromium clients select builds, fight using normal input (Tab to target, a
right drag to face the target since offensive abilities fire where the character faces, #517), observe
a real hit, elimination and the same winner, change class/equipment between lives,
and enter match 2. A client screenshot and the result are written to ignored
`artifacts/browser-battle-royale/`. It does not use a mocked transport, private health
edits, a local PvP simulation or a separate renderer.

The battle-royale #33 run passed the two-browser journey on the fortress against host `d633da2`: the Arcanist won match 1, then Arcanist → Ranger and an empty off hand in match 2. The captured real-client hit below shows both players by the south-west corner tower, the actual target health and spent mana.

![Two real browser players fighting through the hosted MMORPG combat](reviews/battle-royale/pvp-hit.png)

The focused session/targeting tests also cover completed-result retention and safe
player exclusion. No 100-client networking, native mode entrance or large-player
rendering readiness is claimed.

Convention source revision: `6cc05789dab39cee6758b288f83fb48b4ee6dbdb`.

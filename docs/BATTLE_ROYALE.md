# Battle Royale in the MMORPG browser client

## Practice arena (Pages, no server)

`?activity=battle-royale` (or **Battle Royale PvP** from Activity) opens with **Arena:
Practice arena (this browser)** selected unless the URL names an `arenaServer`. Choose a
class, appearance and equipment, then **Start practice**: the page's `mmorpg-wasm` host
(`LocalZone.forPracticeArena`) admits that `EphemeralCharacterBuild` at the maximum level on
the flat arena, facing five stationary **Training Targets** (three about 8–9 m ahead, two at
20 m). Targets are breakable props: they take real `mmorpg-core` damage, never move, fight or
hold threat, break, and stand again after 30 s. There is no storm, matchmaking, team, reward
or other player. **Reset practice (F9)** starts a fresh life with the build chosen at entry;
**Change build** returns to setup to change it. On a phone the touch overlay shows **Target** and
**Attack** from the first moment, and **Menu** offers **Reset practice** and **Change build**;
in portrait the status line sits under the frames and the hotbar above the thumb controls
(#551). Nothing is saved, and the Greyhaven roster,
saves and the hosted path below are untouched. The flat arena is temporary until the shared
fortress (battle-royale #31) is presented (#552).

## Hosted match

Choose **Arena: Hosted match**. Select a class, appearance and equipped
items, then join the same **Match server** as a friend. Characters enter at the
shared supported maximum level, with actual class resources and abilities. Equipment
choices are filtered by the shared item catalog's slots; the host validates them.
This catalog currently has no class or item-level restrictions, and the available
armor is the same for all three classes. Defaults come from `EphemeralCharacterBuild`.

The client reuses its ordinary Three.js renderer, Rust unit presenter, class HUD,
weapons/spells, targeting, shield guard, movement, input bindings and Settings.
Tab and pointer selection include admitted attackable arena players; ordinary safe
players remain excluded. The flat arena uses the same Rust static-scene machinery,
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

Use `battle-royale` revision **f089a978c086b237098deb5a02ffd9c2cd00e600**
(the merged #13 runtime). Its core/protocol pin is MMORPG
`81306faec8a6a7ea3b4279d75f763c1135ca4a19`. Mode protocol/schema/ruleset 2 wrap the
existing MMORPG snapshot v30/schema 29 and command v13. Different versions fail closed.

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
handler. Two actual Chromium clients select builds, fight using normal input, observe
a real hit, elimination and the same winner, change class/equipment between lives,
and enter match 2. A client screenshot and the result are written to ignored
`artifacts/browser-battle-royale/`. It does not use a mocked transport, private health
edits, a local PvP simulation or a separate renderer.

The implementation run passed the two-browser journey, including Arcanist → Ranger and an empty off hand in match 2. The captured real-client hit below shows both players, the actual target health and spent mana.

![Two real browser players fighting through the hosted MMORPG combat](reviews/battle-royale/pvp-hit.png)

The focused session/targeting tests also cover completed-result retention and safe
player exclusion. No 100-client networking, native mode entrance or large-player
rendering readiness is claimed.

Convention source revision: `6cc05789dab39cee6758b288f83fb48b4ee6dbdb`.

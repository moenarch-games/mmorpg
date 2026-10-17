# Save/load and character inspection

## Browser demo persistence

The browser demo runs the shared Rust zone simulation as a local WASM zone host
([ADR 0002](adr/0002-browser-embeds-zone-simulation.md)). Each roster character is a
**durable character** (#30, Atlas 3.1.6): **Enter World** holds the character for this
tab (a Web Lock), leases it from browser storage as local zone 1 at a fresh page epoch
and resumes its committed record, or creates it at the outpost spawn on its first visit.
The record covers class, level, experience, copper, bags, equipment, quests, health,
class resource, running cooldowns and position; casts, auras, target and chat start
fresh. Progress is committed every five seconds, when the page is hidden and when
**Characters** (or **Escape** with world focus) leaves, which also releases the lease.
The roster shows the committed level.

The record and lease rules are the same `mmorpg-characters` rules zone hosts use; the
page only stores entry bytes under `mmorpg.character.v1.<characterId>` and a local zone
epoch under `mmorpg.local-zone-epoch.v1`. A second tab cannot play a character that is
already in play. A page closed or crashed without leaving keeps its last commit, and the
next page's newer epoch supersedes its lease, so the old page can never write again.
Corrupt or other-version entries fail closed instead of being replaced. When the browser
denies storage, characters follow the same rules for the visit only.

The world itself (creatures, corpses, the zone tick) is not saved: issue #40 composes
character records with versioned zone/world checkpoints in an atomic save-slot bundle.
Earlier builds offered **Save game**, **Load game**, **Export save** and **Import save**
for a TypeScript-owned position and waystone flag; those controls are gone.

- the local character roster (`mmorpg.offline-roster.v1`) persists, see
  [character creation](CHARACTER_CREATION.md);
- **Appearance-only saves** (hat style) remain in the collapsed section of character
  selection under `mmorpg.preview-character.v1.<characterId>`;
- old checkpoints under `mmorpg.offline-demo.v1.<characterId>` are neither read nor
  written nor migrated. Their bytes are left untouched, and a character ID that still
  has one, or a stored character entry, is never issued to a new character.

## Turn your character

Drag horizontally over the 3D preview with the primary mouse button or one finger.
One preview-width of dragging is one full revolution. **Turn left/right** rotate by
15 degrees. Focus the preview and use arrow keys, **Home** to face forward, or **End**
to reach 359 degrees. **Reset view** returns to the front-facing view.

Rotation is a presentation-only turntable. All attached equipment follows the same
3D transform; it never changes gameplay facing. The angle stays fixed until user
input, including after changing a hat. A cancelled drag, lost pointer capture, blur,
or switching screens rolls back that unfinished gesture; unrelated pointers cannot
steal or finish it. Completed rotation remains when you return to selection within
the same page session. Reload starts front-facing.

On narrow displays, selection stacks vertically with a dedicated preview above the
controls; vertical touch scrolling is retained. The camera is fitted to that preview
on resize/scroll, without measuring layout in the animation loop.

## Regression and browser evidence

`bun test` covers roster and appearance storage, deterministic turntable input, the
command and snapshot wire fixtures, and the WASM local zone (movement, jumps, scenery,
source contract) and that a character resumes across pages, plays in one tab only and
fences a superseded page. Commits happen every 150 ticks, never per frame.

`python3 scripts/smoke-browser.py` exercises the actual production bundle, renderer
and WASM module: mouse/keyboard/touch rotation, rendered front/reset parity, pointer
cancellation, entering the world, moving, jumping, orbiting, leaving and re-entering,
denied storage, character creation, and mobile layout. It checks that leaving stores the
character, that re-entry resumes it where it left and that no save controls claim to
persist world progress, and it
retains screenshots and a JSON error report under `artifacts/browser`.

Build `web/` first, then install the optional runner:

```sh
python3 -m pip install playwright==1.57.0
python3 -m playwright install --with-deps chromium
python3 scripts/smoke-browser.py
```

The Pages workflow reuses its one production build for browser acceptance on main,
manual runs, or PRs carrying **browser-evidence**. Ordinary PRs keep the cheaper
unit/typecheck/build path; no second build or general-purpose CI job is added.

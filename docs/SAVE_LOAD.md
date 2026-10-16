# Save/load and character inspection

## Browser demo persistence

The browser demo runs the shared Rust zone simulation as a local WASM zone host
([ADR 0002](adr/0002-browser-embeds-zone-simulation.md)). World state (position,
facing, tick) lives in that simulation, so the page **does not save world progress**.
**Enter World** spawns a new character at the outpost spawn; **Characters** (or
**Escape** with world focus) leaves the zone and removes the unit. Nothing is written
to browser storage while playing.

Earlier builds offered **Save game**, **Load game**, **Export save** and **Import save**
for a TypeScript-owned position and waystone flag. Those rules no longer exist, so the
controls are gone rather than claiming to persist state they cannot restore. Durable
progress (class, level, experience, inventory, quests, position) belongs to a character
record behind core command/query APIs: issue #30. Issue #40 composes that record with
versioned zone/world checkpoints in an atomic save-slot bundle and restores the
matching character and world. A character record alone does not re-enable world
progress saves. Until the composed save/load flow exists:

- the local character roster (`mmorpg.offline-roster.v1`) persists, see
  [character creation](CHARACTER_CREATION.md);
- **Appearance-only saves** (hat style) remain in the collapsed section of character
  selection under `mmorpg.preview-character.v1.<characterId>`;
- old checkpoints under `mmorpg.offline-demo.v1.<characterId>` are neither read nor
  written. Their bytes are left untouched, and a character ID that still has one is
  never issued to a new character.

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
source contract). There are no per-frame persistence reads or writes.

`python3 scripts/smoke-browser.py` exercises the actual production bundle, renderer
and WASM module: mouse/keyboard/touch rotation, rendered front/reset parity, pointer
cancellation, entering the world, moving, jumping, orbiting, leaving and re-entering,
denied storage, character creation, and mobile layout. It checks that playing writes
no browser storage and that no save controls claim to persist world progress, and it
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

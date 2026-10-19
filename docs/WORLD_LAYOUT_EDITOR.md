# Greyhaven layout authoring with Blender

Blender 5.2.2 LTS is an **optional development-only program** for arranging the
Greyhaven Outpost. It replaces the former HTML editor. The game never loads
Blender, .blend files or Python scripts. The repository's existing
`content/greyhaven/outpost-layout.json` is the authoritative saved
authoring input. No in-game editor or second world simulator is introduced.

## Start editing

Install [Blender 5.2.2 LTS](https://www.blender.org/releases/5-2/) on your
development machine and make `blender` available on PATH, or set
`BLENDER_BIN` to the installed executable. Rust and Cargo must also
be installed. The launcher checks the exact Blender version rather than
installing or updating it.

From the MMORPG repository root:

```sh
bash scripts/dev-blender.sh open
```

On the first run, a small native Rust example exports the **actual** current
Outpost scene context from `mmorpg-core` and `mmorpg-scenery`.
Blender opens a generated `artifacts/greyhaven/outpost.blend` (ignored
by Git) with separate collections:

- **Greyhaven Editable**: four existing houses, saved lamps and authored
  decorations. Use Blender's ordinary select/move/duplicate/delete tools,
  object transforms and Outliner.
- **Greyhaven Context**: a terrain mesh, roads, buildings and nearby scenery
  derived from the real game. This is a non-selectable guide; its geometry is
  not an editable source of gameplay.
- **Greyhaven Prefabs**: one house, lamp, signpost and flowers template.
  Duplicate a template, move the copy into **Greyhaven Editable** using
  Blender's Collections/Outliner, and position it inside the Outpost region.

Blender uses metres and Z-up. Game X maps to Blender X, game Z maps to negative
Blender Y, and game Y maps to Blender Z. Only horizontal position and yaw
(for decorations) are supported by the saved layout; the existing house
colliders remain axis aligned. Changes to mesh topology, scale or height
cannot be represented by this file; model assets separately in Blender and
pass them through the existing asset-tooling GLB path.

## Save changes into the game

1. In Blender, save the .blend project and **close the Blender window**.
2. From the repository root, run:

   ```sh
   bash scripts/dev-blender.sh export
   ```

3. The bridge serializes the edited placements and runs the real Rust
   `validate-layout` example, which checks core
   collision/spawns/roads and presentation overlap. Only accepted layouts
   atomically replace `content/greyhaven/outpost-layout.json`.
   Rejected edits leave the repository file intact.
4. Rebuild/restart the game. To refresh the browser's compiled WASM:
   `cd web && bun run build:wasm`. Native host/client builds
   consume the same committed layout, without Blender installed.
5. Run `bash scripts/dev-blender.sh open` to continue editing.
   The accepted project retains generated object IDs and is synchronized
   to its saved layout.

After a failed validation, reopen the saved .blend, correct the offending
objects and export again. The export rejects any external changes to the JSON
since the Blender project was opened, rather than overwriting them.

This first slice can add, delete and move Outpost houses, lamps, signposts and
flowers. Other generated scatter, structures, terrain sculpting, physics
changes outside the supported house layout, and model import remain separate.

## Ownership and format

- The existing `version: 1`, `generator: "greyhaven-15"`
  JSON representation, stable collider IDs (103–106, authored 60000–64999),
  tombstones and persistent decoration IDs are unchanged.
- `mmorpg-core` validates physical colliders; `mmorpg-scenery`
  owns decoration and suppresses intersecting procedural scatter. Both
  browser and native clients consume the same accepted definition.
- A generated Blender .blend is local, disposable authoring state under
  `artifacts/`, not the canonical source. A clean checkout rebuilds
  it from the checked-in JSON and deterministic game content. Never overwrite
  a .blend containing unsaved work merely to regenerate it.
- The adapter runs only from the developer command, outside rendering,
  simulation, network hosts, production builds and default CI.

## Focused verification

```sh
python3 -m unittest discover -s scripts/blender -p 'test_*.py'
cargo run --locked -p mmorpg-scenery --example validate-layout -- content/greyhaven/outpost-layout.json
# With Blender 5.2.2 available, manually verify an edit-save-export-rebuild cycle:
bash scripts/dev-blender.sh open
bash scripts/dev-blender.sh export
```

This supersedes the browser-editor implementation described in the original
TECH-010 decision, while retaining TECH-008/TECH-009's Blender authoring
and non-destructive override policy.

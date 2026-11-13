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
- `mmorpg-core` parses and validates the whole layout. Houses and the solid
  decorations (lamps and signposts) compile into colliders (#577), so moving
  one changes the content revision; flowers stay walk-through.
  `mmorpg-scenery` draws every decoration where core put it and suppresses
  intersecting procedural scatter. Both browser and native clients consume
  the same accepted definition.
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

## Painting terrain and biomes in Krita

Blender places objects; Krita paints the ground. The `mmorpg-world-studio`
program writes one layered OpenRaster document per named area and brings the
painted layers back into `content/greyhaven/paint/`. Capturing needs a built
`web/dist` and Chromium (`mise run setup:browser`, or `WORLD_STUDIO_CHROME`):

```sh
alias studio='cargo run -q -p mmorpg-world-studio --'
studio capture --list                 # the areas and their sizes
studio capture --only outpost         # one area, to artifacts/regions
studio capture --tiles                # all of them, plus a map tile pyramid
# Paint in Krita and save the .ora, then:
studio import artifacts/regions/farmland-and-hamlets.ora --check
studio import artifacts/regions/farmland-and-hamlets.ora
```

Do not resize or crop a document: its hidden `_bounds` layer records where it
lies in the world. Images are orthographic and north-up at noon, without units,
fog or interface, through the `?debug` hook `planView`, which never touches the
simulation. `+X` runs right and `+Z` down: `world_x = min_x + pixel_x /
pixels_per_metre`, and likewise for Z and rows. `manifest.json` records every
region's `bounds_metres` and the biome palette.

| Layer | Use |
| --- | --- |
| `beauty`, `ground` | The rendered world, and the bare terrain alone |
| `hillshade`, `height`, `slope` | Reference from the physical ground |
| `biome`, `roads`, `water` | Reference from the shared scenery |
| `locks` | Red where a painted height is ignored |
| `paint height` | Grey 128 is no change, one grey step is 10 cm (−12.8 to +12.7 m); transparency fades towards no change |
| `paint biome` | Palette colours from `manifest.json`; transparent keeps the generated biome |

Each terrain sample covers a 4 m square, and the averaged paint of that square
decides it, so detail finer than 4 m is lost. An import replaces what the
document covers rather than adding to it: erasing paint restores the generated
ground, and re-exporting already carries the committed paint.

`paint height` offsets the real terrain, so physics, trees and both clients
follow, and the content hash changes on its own. It cannot move ground under or
beside structures, near water, on the flat Outpost or in the cave and shrine:
the importer skips such samples and `cargo test -p mmorpg-scenery` rejects them.
`paint biome` is presentation only; roads, water and the plaza always win over it.
Each region's folder also holds these layers as PNGs, with a 16-bit `height.png`
at one pixel per sample (value = height in cm + 32768).

[World Studio](WORLD_STUDIO.md) wraps this workflow in a local web app.

`--tiles` renders the whole zone as an XYZ raster pyramid under `tiles/`, with
`tiles/style.json` for the `mapStyle` of `@moritzbrantner/maps`. The pyramid's
whole Web Mercator world is a 2048 m square centred on the origin; the conversion
is in `manifest.json`. Set the served path with `--tiles-url`.

## Outpost kit dressing

The validated Blender export also writes `content/greyhaven/dressing/outpost.json`
from the candidate layout before either file is replaced. Review and commit both
files together. It preserves the existing closed-building gameplay collision;
`fallbackCollider` only hides each blockout while the kit is drawn, and `wear`
is cosmetic. Clients read the committed placements through shared world dressing,
without composing module transforms from collider footprints at runtime.

The house and smithy module compositions live in
`content/greyhaven/authoring/outpost-buildings.json`. Change those data templates
rather than Rust renderer code when editing a prefab's art. After a manual layout
or template edit, regenerate the dressing explicitly and review its diff:

```sh
cargo run --quiet --locked -p mmorpg-scenery --example export-outpost-dressing > content/greyhaven/dressing/outpost.json
```

The command accepts an optional layout path for offline candidate generation.
It emits persistent per-building/module IDs, exact unit transforms and the original
wear values. The committed dressing is its generated output; make durable prefab
edits in the authoring templates rather than editing that output before an export.
Art regeneration never changes authoritative collision. The editor's existing
layout JSON and core validator contract remain unchanged.

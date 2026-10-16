# Saved Greyhaven grass authoring

The first saved-mask package freezes 118 already accepted `grass-tuft` placements
from the original Greyhaven Outpost scenery at MMORPG revision
`166a003c067208234d3a484e1841a0ff49c74ef3`. The package lives in
`crates/mmorpg-scenery/assets/outpost-grass/`. The selected package is consumed by native and browser clients through shared
scenery. Authoring stays offline; no filter runs in either render loop.
Gameplay content, colliders, RNG and snapshots are unchanged by this package.

`accepted.instances.json` retains stable ordered IDs and complete local XYZ
coordinates. `transforms.json` retains each accepted yaw, scale and body extent.
These are deliberate frozen source inputs, not a scatter recipe to rerun.
Source positions came from existing scenery grass, with its presentation relief
removed to recover the original ground anchor at Y = 0. Relief remains scenery
owned and is applied once by clients. `source.json` records the original source
commit, scenery export SHA-256, scenery hash and gameplay content identity.

## Grid and world calibration

The right-handed Y-up instance set uses one million micro-units per metre.
One game unit is one centimetre. Local XYZ converts to game units by dividing
by 10,000, then adding XZ origin `[0, 2000]`; Y has no offset. The inclusive
world footprint is X = −3500…3500 and Z = −1300…5300 game units.

The source bounds are deliberately odd: width 70,000,001 and depth 66,000,001
micro-units. Their centered inclusive endpoints are exactly ±35 m and ±33 m.
The saved mask has 71 columns along +X and 67 rows along +Z, so samples lie
exactly one metre apart. The first sample is world (−35 m, −13 m); the last
is (35 m, 53 m). Nearest endpoint sampling rounds midpoint ties to the larger
index. No implicit stretch or source-set offset is passed to the producer;
the declared game-space origin is applied only by the scenery consumer.

`exclusion.txt` is the independently editable saved mask: `.` means keep (0),
`X` means exclude (255). Every row must have exactly 71 glyphs and end with a
newline. The build encodes opaque grayscale RGBA8 and explicitly declares
linear scalar data sampling. A maximum coverage of 127 keeps black samples.
The mask widens the Outpost road corridors to 2.5 m from their center lines,
the spawn plaza by 1 m and structure footprints by 1 m. The deliberate western
clearing spans world X = −28…−18 m, Z = 8…16 m. These are cosmetic clearances;
no mask removes a structure, tree trunk or other gameplay collider visual.

## Edit and reproduce

Use a **clean** asset-tooling checkout at
`1e79d74ee0a62cd29706ed3254a0293c91d996ba` (PR #163), then run from the MMORPG root:

```sh
bun web/scripts/package-grass.ts /absolute/path/to/asset-tooling --check
# After deliberately editing exclusion.txt:
bun web/scripts/package-grass.ts /absolute/path/to/asset-tooling --write
(cd web && ASSET_TOOLING_MASK_SOURCE=/absolute/path/to/asset-tooling bun test tests/grass-package.test.ts)
(cd web && bunx --no-install tsc --noEmit -p tsconfig.assets.json)
```

The adapter resolves only the producer's declared public package exports and
uses `instances.filter.exclusion-mask@1`; MMORPG implements no production
sampling/filtering kernel. The source checkout is checked before importing it.
Each build uses disposable content-addressed stores, repeats the filter, and
compares a cold replay. No source candidates are generated or backfilled.

The deliberately packaged `selected.instances.json`, `selected.transforms.json`
and `exclusion.rgba8.json` retain the original selected IDs/coordinates/transforms.
The initial mask keeps 55 of 118 instances. `manifest.json` records every source
and output hash/length, complete AssetRefs, calibration and the producer's build
identity including its source fingerprint. `--check` reproduces every output;
`--write` reconciles them after an intentional source/mask edit. Neither command
modifies the accepted source set. Clearing the mask restores that entire source;
restoring a prior mask reproduces its selected package byte for byte.

Ordinary tests verify packaged identities and an independent fixture sampling
oracle without a producer checkout. Setting `ASSET_TOOLING_MASK_SOURCE` additionally
runs public producer replay, localized edit/undo, empty/full masks and invalid
calibration/provenance checks. Pages CI checks out the exact producer revision,
reproduces the package, and runs these cases. Tests never silently substitute a
consumer filtering implementation for producer evidence.

## Shared scenery consumption

The adapter also emits `accepted.props.rs` and `selected.props.rs`, exact
centimetre coordinates and original transforms suitable for Rust inclusion.
The manifest records their hashes plus the consumer adapter's own source hash
and its checkout/public-export helper dependencies. The relief authoring
package shares this small public producer boundary.
Sub-centimetre coordinates fail closed rather than being rounded. The selected
include is compiled into `mmorpg-scenery`; the accepted include supplies the
independent original-placement regression check. No JSON parser or asset-tooling
runtime dependency enters scenery or network hosts.

`outpost_grass_placements()` exposes stable saved instance IDs and ground-anchor
props. The scenery builder replaces only cosmetic grass roots inside the
inclusive authoring footprint with that selection. It preserves the original
placement RNG execution for every other family. Tests retain the original
scenery hash and compare every accepted placement, unrelated prop, road, water
surface and both terrain grids against the original builder. The grass-only
checksum is `1de3341c933449f6`; adoption of the independent
[saved relief package](RELIEF_AUTHORING.md) makes the current scenery regression
checksum `d7b2eda257de90ab`. Gameplay stays at content
revision 4.

Scenery export v3 adds `presentationFingerprint`, a 16-digit hexadecimal FNV-1a
hash of the complete deterministically serialized export, with its own field
empty during hashing. This includes the actual 4 m near and 20 m far grids,
all prop/structure records, palettes and other presentation fields. The current
export fingerprint is `9536a65a74d1220b`. Browser static, water and animated
resource keys include it, so
mask edits cannot reuse geometry under an unchanged gameplay revision. Older
scenery exports fail closed; gameplay snapshot/command wire stays v7/v2.
Native static props remain instanced boxes; browser grass joins the existing
chunk/colour/distance batches. The debug `grass` camera shows the western
clearing without moving the player. Browser acceptance saves its frame and
bounded scene-work evidence under `artifacts/browser/outpost-grass-*`.

The authoring captures and adoption checksums above describe revision 4. Corpse
loot activation (revision 5), class abilities (revision 6), equipment
(revision 7), the vendor (revision 8) and quests (revision 9) change only live
content identity metadata: Greyhaven revision 9, fingerprint `2630ca15716973f3`,
snapshot v14/command v9. Live scenery hash is `90683364fa48a752` and complete
browser export fingerprint is `3d83424fd061c312`.
All authored masks, placements, heights and source revision-4 provenance remain
unchanged; none of these identity changes requires recapturing geometry.

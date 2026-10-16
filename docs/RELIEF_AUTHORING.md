# Saved Greyhaven relief authoring

The Outpost package in `crates/mmorpg-scenery/assets/outpost-relief/` freezes
the current shared presentation heights and a separately editable flatten mask.
Both clients consume its saved flattened result through shared scenery.
Gameplay, colliders and snapshots retain their current identities.

## Source and calibration

`source.heights.json` contains 18,753 signed integer centimetre heights captured
from the public WASM `reliefAt(x,z)` query at MMORPG commit
`6265e1c842aad9d2034bfe7946dec4582158b9e6`. This is the existing shared scenery
output, including its original analytic clearances, rather than an unflattened
noise field. `source.json` records the exact source commit, complete scenery
export SHA-256, presentation fingerprint and gameplay content identity.
It also records a separate immutable capture digest for the source heights;
accidentally changing an in-range sample fails before generating any outputs.

To reproduce the capture, build that source revision's browser WASM adapter,
initialize its generated module, and query `reliefAt(-3500 + column * 50,
-1300 + row * 50)` for each row 0…132 and column 0…140. Store the returned
signed integer heights in row-major order with the declared grid metadata.
The saved source is deliberate authoring input; normal package builds never
recapture it from a later live client or alter it during filtering.

The non-square grid has 141 columns along +X and 133 rows along +Z. Its origin
is world XZ = [−3500, −1300] centimetres, step = 50 centimetres, and inclusive
last sample = [3500, 5300]. The center sample is [0, 2000]. Y is centimetres
above the existing flat gameplay ground; it has no world-origin offset.
Source heights range from −36 to +25 cm and must stay within the shared
presentation range −60…+60 cm.

Each signed height is encoded as an opaque grayscale byte `height + 128`.
One byte step equals one centimetre, so encoding loses no precision and the
producer target byte 128 maps exactly to height zero. Height and coverage
channels are explicitly linear scalar data; the RGBA8 container's color tag
does not request a gamma transform.

## Saved coverage and boundaries

`flatten.txt` contains exactly 133 newline-terminated rows of 141 glyphs:
`.` = 0, `1` = 64, `2` = 128, `3` = 192, and `X` = 255. Full coverage extends
1.5 m beyond selected building footprints and the spawn plaza, and 2.5 m from
road center lines. Three half-metre rings soften the exterior transition.
The outer two samples along every edge have zero coverage, preserving source
boundary values for bounded adoption. Changing them fails closed.

The initial saved mask changes 4,517 samples. Zero coverage retains source
heights, full coverage reaches zero, and partial coverage uses the producer's
deterministic integer blend. Production MMORPG code only encodes the inputs
and decodes signed output heights; it contains no flattening kernel.

## Edit and reproduce

Use a clean asset-tooling checkout at
`1e79d74ee0a62cd29706ed3254a0293c91d996ba`, then run from the MMORPG root:

```sh
bun web/scripts/package-relief.ts /absolute/path/to/asset-tooling --check
# After deliberately editing flatten.txt:
bun web/scripts/package-relief.ts /absolute/path/to/asset-tooling --write
(cd web && ASSET_TOOLING_MASK_SOURCE=/absolute/path/to/asset-tooling bun test tests/relief-package.test.ts)
(cd web && bunx --no-install tsc --noEmit -p tsconfig.assets.json)
```

The adapter invokes only the pinned public `image.height.mask-flatten@1`
operation. Grass and relief share the small checkout/public-export boundary;
their authoring pipelines and saved inputs remain independent. Each build
uses disposable content-addressed stores, repeats the operation and compares
a cold replay. Neither `--check` nor `--write` modifies source heights or masks.

The generated source, coverage and flattened RGBA8 containers, plus
`flattened.heights.json`, are checked in. `manifest.json` records every input
and output hash/length, complete AssetRefs, calibration, operation build/source
identity, and hashes of the consumer adapter and its shared helpers.

Ordinary tests validate all package identities, signed-height encoding,
non-square endpoint mapping, retained boundary samples and an independent
test-only blend oracle. With `ASSET_TOOLING_MASK_SOURCE`, tests also reproduce
all bytes through the public producer, edit one mask cell, undo that edit,
exercise zero/full interior coverage and reject invalid pins/calibration.
Pages CI checks out the exact producer, reproduces both saved packages and
runs these cases. The accepted grass selection is unchanged.

Writes stage and verify the complete generated output set before replacing
changed files with atomic same-filesystem renames. Identical files retain
their inode and timestamp. The manifest is replaced last, and a caught
replacement error rolls back files already replaced. An interruption between
file replacements can leave a mixed generation; `--check` detects it and a
successful `--write` reconciles it. Individual durable files are never partially
written.

## Shared client consumption

The adapter also emits `flattened.heights.rs` and `source.heights.rs`, including
the field's exact origin, step, dimensions and signed-centimetre samples. The
first is compiled into shared scenery; the second supplies the independent
original-capture regression in tests. No JSON parser or asset-tooling operation
runs in either client.

`Scenery::height_at` samples this field inside its inclusive Outpost footprint
and retains the original relief outside. Integer bilinear sampling rounds to
the nearest centimetre, with half ties toward +Y. Grid nodes exactly reproduce
the saved values. The untouched edge samples match the old function exactly
at nodes; between nodes they reconstruct it within one centimetre, with no
adjacent-coordinate seam jump above two centimetres. Procedural placement runs
before adoption, preserving every existing prop and random draw.

Native two-metre terrain samples, browser four-metre terrain samples, exported
far samples and unit/prop relief queries use this shared owner. The authored
half-metre field changes 4,517 nodes; the scenery regression checksum becomes
`d7b2eda257de90ab`, and the complete browser export fingerprint becomes
`9536a65a74d1220b`. Existing resource keys include that fingerprint. Gameplay
remains content revision 4, fingerprint `5738a86de795e940`, snapshot v7/command v2.

The debug `relief` and `hub` camera poses show the plaza, road and building
approaches without moving the character. Browser acceptance records screenshots
and scene-work/flat-approach evidence under `artifacts/browser/outpost-relief-*`.
Native smoke explicitly renders the same poses on the GPU; setting
`MMORPG_SMOKE_FRAME_DIR` saves their PPM frames. Tests verify every source and
selected node, shared WASM/grid consumption, unchanged surrounding scenery,
clearance flatness and centimetre boundary continuity.

The authoring captures and adoption checksums above describe revision 4. Corpse
loot activation (revision 5), class abilities (revision 6), equipment
(revision 7), the vendor (revision 8) and quests (revision 9) change only live
content identity metadata: Greyhaven revision 9, fingerprint `2630ca15716973f3`,
snapshot v14/command v9. Live scenery hash is `90683364fa48a752` and complete
browser export fingerprint is `3d83424fd061c312`.
All authored masks, placements, heights and source revision-4 provenance remain
unchanged; none of these identity changes requires recapturing geometry.

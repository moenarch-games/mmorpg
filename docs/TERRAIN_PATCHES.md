# Southmeadow terrain patch

Issue [#782](https://github.com/moenarch-games/mmorpg/issues/782) adopts the public
`asset-tooling/recipes/terrain-patches` recipe for a bounded Southmeadow Downs
patch: X = 80–112 m, Z = 164–196 m. This is east of the ruined farmstead,
south of the Vale Ring Road, and outside Millbrook Farm and every crop field.
The bounds are an implementation choice under the issue's one-area scope.

`content/greyhaven/terrain-patches/southmeadow/recipe.json` owns the saved inputs.
The recipe's Q8 gravel/meadow paint and slope masks drive the existing
`greyhaven-terrain` gravel and rock overlays through shared presentation. An
excluded border feathers the patch into the existing surface. Linear bilinear
sampling uses one-metre samples, columns along +X and rows along +Z.

The recipe scatters real `greyhaven-meadow` grass and flower pieces and small
`river-stones` bed fragments. The latter are cosmetic loose ground debris, not
new freestanding solid rocks. The owner explicitly approved “Allow tiny flush
cosmetic fragments” for #782: these fragments need no new colliders; larger
rocks retain the collision requirement in Atlas 5.1.i577 (see 5.1.i782-2). The adapter preserves the producer's
`layer:cellX:cellZ` IDs under the `southmeadow:` area prefix, variant, XZ,
yaw and scale (centimetre positions, turn/65536 yaw and thousandth scale) in `content/greyhaven/dressing/southmeadow.json`. Both clients
use the same world-dressing transforms and existing bundle/LOD loaders.
The shared selection stops drawing scatter beyond 60 m from its origin,
independent of scale or bounding radius; no far cards remain.

This adopts **masks and detail only**. Recipe height is used only to derive
its cosmetic slope and distribution; neither the recipe mesh nor its heights
are drawn as a floor. Detail roots sample the existing shared terrain mesh.
Core terrain, authored height/biome paint, collision, roads, crop rows,
procedural RNG and all other placements are unchanged. Existing road, water
and crop clearances also reject ground detail at consumption.

## Reproduce

The authoring adapter uses the public export from the immutable asset-tooling
source archive at `b2166ba96285ed36093e9c71aeb38907d1218635`. It never imports
a mutable authoring checkout. With that archive unpacked and its frozen Bun
dependencies installed, run from the MMORPG root:

```sh
bun web/scripts/package-terrain-patch.ts /path/to/asset-tooling-b2166ba96285ed36093e9c71aeb38907d1218635
```

The default path is the existing `~/.cache/mmorpg/ranger-tools/asset-tooling-<revision>`
source cache. This merged revision has the same source tree as the originally
validated `8a22d6950d00d881500472ff781dfc5b874a0831`; changing that identity
does not change generated masks or placements. The command writes the saved masks and dressing; these small
runtime distribution artifacts are committed, so no producer or generation
runs in the clients or hosts. Repeating the command preserves identical bytes
and IDs. `scripts/fetch-bundles.sh` acquires the published meadow and river-stone
art. No new asset generation, parser or renderer is involved.

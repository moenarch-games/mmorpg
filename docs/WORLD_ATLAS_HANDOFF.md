# Continent atlas: progress and handoff

Branch `agent/continent-atlas`. Not merged; it waits for owner review. The design is in
[WORLD_ATLAS.md](WORLD_ATLAS.md).

## State (2026-11-10, after the second owner round)

Done:

- **Audit.** Greyhaven is one zone on a 4 m height grid in centimetres, with −Z as north. Its
  terrain, landscape JSON, paint layers and Blender meshes feed collision and the content
  revision hash. World Studio paints per area, physics tiles are internal, and the repository
  has no continent, world map, Crown or kingdom lore.
- **Source layers** in `content/world/`: terrain, water, anchors, climate, regions,
  polities and two override layers.
- **Generator** `content/world/authoring/atlas.py`: deterministic elevation with ranges and hill country, river carving with
  meanders, priority-flood hydrology with sinks and a karst link, westerly and southern
  rain with rain shadow, biomes, 151 terrain-grown regions with overlapping borders, kingdom
  claims, routed roads, previews, the report and the adjacency table, and the checks
  (60 passing).
- **Blender round trip** `content/world/authoring/atlas_blender.py` (whole continent, or one file per region), with the headless test `test_atlas_blender.py`. Verified on
  Blender 5.2.2: an untouched round trip changes nothing, and moving one seed changes one line.
- **Previews** in `docs/images/world/` and the design document.

Not done, or limited:

- No game crate reads the atlas. The minimal "continuous composition" demonstrator is the
  generator's overlapping-region composition with its chunk-equivalence check, not a runtime
  integration. Broad terrain replacement was out of scope, and wiring the far field into
  `mmorpg-scenery` is step 2 of the plan.
- No Decision Atlas entries: no issue exists to derive an ID from. Record accepted parts after
  owner review.
- Regions are unnamed except Greyhaven Vale, Crownhollow, Saltmere Pan and Gull Isle.
  Names and level bands are deferred.
- The climate and biome model is coarse, on a 40 m grid with compressed elevations; it is
  meant for placement, not art.
- The Greyhaven stand-in elevations are coarse; the zone's own height grid was not sampled.

## Resume

```sh
git switch agent/continent-atlas
uv run content/world/authoring/atlas.py check --previews docs/images/world
```

`check` builds twice (reproducibility) and takes about a minute. Edit the JSON by hand or
through Blender, rerun `check`, and commit the sources with the refreshed previews. Never
regenerate region seeds wholesale once someone has edited them: `seed-regions` only appends.

## Log

1. Audited the authoring stack; found no lore for the Crown or the kingdoms. Treated
   Greyhaven's passes as the established entrances (Question 1 in the design document).
2. Built the layers and the generator. Fixed, in turn: capitals placed in bays, a flooded
   crater, square carve artefacts, planar valley facets, straight canyons, and rivers escaping
   shallow gutters (beds now lie below the lowest ground across the valley).
3. Recalibrated rain from a percentile-normalised index to a physical rain-out model, so
   Greyhaven's woods and farm fall in temperate woodland and the east is a rain shadow.
4. Sized regions to the owner's target of about 150 at 2–4 km².
5. Added the Blender round trip and a canonical JSON layout.
6. An independent review found that the wetlands drained into the Vale, roads ignored the
   established passes, river heads erased the gate floors, several checks could not fail,
   Blender duplicates and object moves lost edits, rain shadows striped every ridge, and the
   geology contradicted itself. All of these are fixed:
   - The anchor now models the ridges between the Vale and its outer regions, and each part
     declares its outlet.
   - Roads start at the declared exits.
   - River beds start at the ground at their heads.
   - The checks test the result instead of construction (45 checks).
   - The Blender export uses world space, treats copied seeds as new regions, refuses other
     copies, and has a headless test.
   - The rain shadow follows the upwind barrier.
   - The Crown is described as a structural dome with a limestone apron.
   - Dependencies are locked, overrides are validated, ids are never reused, and wind
     directions are read from `climate.json`.

   Not done from the review:
   - The CI path filter still treats `content/world/**` as a Rust change.
   - No cross-platform reproducibility evidence beyond Linux x86-64.
   - The two derived lakes it pointed out remain unnamed.
7. Owner round 2: the continent is as big as a small Europe and should read like one, with
   more ranges, hills and rivers, a cut-back south-west with islands, and no desert.
   - Ranges: the Teeth raised, and the Isen Fells reshaped; new ranges are the Rimeback,
     Corrie Heights and the Merrow Spine; the Sedge Downs replace the Vess Downs.
   - Hill country: a new `uplands` layer of six rolling hill countries, also movable in Blender.
   - Rivers: fifteen instead of six. Tributaries are carved first and run on to the main
     river's meandering course; a last pass keeps every course from rising.
   - Lakes: hollows under 18 m silt up (`water.json` `silting`), so the noise leaves no ponds.
   - Coast: the south-west is cut back into an archipelago of seven isles. Islands may be
     stretched and rotated and get irregular coves, and each has a pinned claim.
   - No desert: the salt pan and dry scrub are gone. The Saltmere is now a reed lake on a
     steppe-grass plain, a weak easterly wets the far east, and the Sarr reaches the sea
     through the Teeth. New biomes are heath and moorland, and maquis on the warm south coast;
     cold dry ground is taiga.
   - Regions: the five seeds that fell into the sea were deleted, and island seeds were added.
     Stray fragments of a region now join their neighbours.
   - The river drainage check counts only samples on land.
8. Owner round 3: regions as puzzle pieces, each with its own Blender file, and a border is
   changed by opening the two regions that share it.
   - `borders.json` (layer 8) is now the source of region shapes. It holds 383 borders, each
     shared by exactly two regions, and 240 junctions. Regions are the pieces the borders cut
     the land into, with one seed per piece.
   - The borders were traced once from the grown regions (`atlas.py borders`), rounded with a
     majority filter, and smoothed; junctions closer than 150 m were merged. Retracing needs
     `--replace`.
   - Four new checks: one piece per seed, borders end at their junctions, each border has its
     two named regions on either side, and the anchor lies inside its region (59 in all).
   - Blender: `region <ids…>` builds a file for one region or several neighbours, and
     `all-regions` builds a file for every region (about 15 s, 46 MB under artifacts/, never
     committed). Export writes only what the file changed, keeps borders ending at their
     junctions, and refuses to overwrite a border changed elsewhere since the file was made.
     The headless test grew to 11 cases.
   - Not done: splitting or merging regions in Blender. A copied seed still adds a region,
     but the check then fails until a border separates it. Region terrain pieces follow the
     border at the atlas's 40 m resolution only.
9. Owner round 4: push on the limits, and try the workflow in a real Blender.
   - Splitting and merging regions in Blender. To split, draw a new border in the Borders
     collection and Shift+D the seed; its ends connect to junctions or split the borders they
     meet. To merge, delete a border and a seed. The region logic now lives in
     `atlas_regions.py`, shared by the generator and Blender.
   - Every export is all or nothing. It runs the same consistency step as the new
     `atlas.py sync` (region pairs, joining borders, dropping unused junctions) and writes
     nothing when there are problems. After a split or a merge the file marks itself out of
     date. A new check, `borders.synced`, makes 60.
   - Terrain pieces: the corners a piece shares with a neighbour are moved onto the nearest
     border line, so the pieces follow the smooth borders without gaps.
   - Region files are centred on their regions (scene origin); the export maps back.
   - The live trial in Blender 5.2.2, on a scratch copy, found a real bug: the Draw tool makes
     Bézier curves with few control points, which the export had flattened. It now follows the
     curve, and redrawn ends near a junction are pulled onto it without a hook. The headless
     test has 15 cases, including a real merge and split that then pass the generator's checks.
   - Not done: deleting a junction marker does nothing (junctions follow the borders); a split
     drawn across more than one region is not supported (it reports the problem instead).
10. Owner round 5: GIS compatibility. The atlas stays its own JSON. Two new commands,
    `gis-export` and `gis-import` (`atlas_gis.py`), exchange it with GIS tools.
    - Two frames: EPSG:3857 metres, with the world placed at 0° N 0° E, and WGS84 per RFC 7946.
    - Region topology as TopoJSON with shared arcs, and the same regions as plain GeoJSON
      polygons.
    - GeoJSON layers: borders, junctions, seeds, settlements, rivers, river courses, ridges,
      uplands, water points, gates, roads and coastline.
    - The import is all or nothing and matches features by id. A border or seed without an id
      is new (a split). Deletions are not supported, and a folder older than the sources is
      refused.
    - Validated independently: `topojson-client` decodes 151 valid, non-overlapping
      polygons covering the land. The test `test_atlas_gis.py` checks the topology, the
      untouched round trip, the stale refusal, a junction move and a split made in WGS84.
11. Owner round 6: a world map in the game, opened with M in both clients and drawn with the
    Maps library. Branch `agent/world-map`, stacked on the atlas PR.
    - `atlas.py map-export` commits `content/world/export/world-map.json`. The new check
      `export.map` keeps it in step with the sources (61 checks).
    - Shared rules in `mmorpg-client-model::world_map`: the map, zone point to map point,
      facing to bearing, and region lookup. The browser reaches them through `mmorpg-wasm`.
    - Browser: `web/src/world/world-map.ts`, Maps' `MapView` on its first-party flat runtime
      with no tiles. Vite must not pre-bundle `@moritzbrantner/maps/wasm`.
    - Native: `crates/mmorpg-client/src/ui/world_map.rs`, an egui window drawn with
      `maps-core` (`MapCamera`, `LineMesh`) and earcut. Its evidence frame comes from the
      native interface GPU test.
    - Borders near the zone footprint are now smoothed unless they touch it.
    - Not done: map labels for kingdoms and regions (only settlements, gates and
      landmarks are labelled); other players on the map; a gamepad binding for M.

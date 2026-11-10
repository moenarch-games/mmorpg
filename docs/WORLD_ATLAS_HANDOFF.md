# Continent atlas: progress and handoff

Branch `agent/continent-atlas`. Not merged; it waits for owner review. The design is in
[WORLD_ATLAS.md](WORLD_ATLAS.md).

## State (2026-11-10)

Done:

- **Audit.** Greyhaven is one zone on a 4 m height grid in centimetres, with −Z as north. Its
  terrain, landscape JSON, paint layers and Blender meshes feed collision and the content
  revision hash. World Studio paints per area, physics tiles are internal, and the repository
  has no continent, world map, Crown or kingdom lore.
- **Source layers** in `content/world/`: terrain, water, anchors, climate, regions,
  polities and two override layers.
- **Generator** `content/world/authoring/atlas.py`: deterministic elevation, river carving with
  meanders, priority-flood hydrology with sinks and a karst link, westerly and southern
  rain with rain shadow, biomes, 149 terrain-grown regions with overlapping borders, kingdom
  claims, routed roads, previews, the report and the adjacency table, and the checks
  (38 passing).
- **Blender round trip** `content/world/authoring/atlas_blender.py`. Verified headless on
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

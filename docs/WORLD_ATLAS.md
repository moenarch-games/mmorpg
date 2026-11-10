# Continent atlas

A provisional, editable geographic foundation for the whole world: one continent of about
20 × 20 km around Greyhaven, the Shattered Crown and five competing kingdoms. It is
**development tooling and design data only**: no game crate, host, client or content revision
reads `content/world/` yet, and Greyhaven's zone content stays the authority for everything
inside its footprint. Progress, open gaps and how to resume are in
[WORLD_ATLAS_HANDOFF.md](WORLD_ATLAS_HANDOFF.md).

![The continent](images/world/world-map.jpg)

```sh
uv run content/world/authoring/atlas.py check --previews docs/images/world   # build, render, verify (about 1 min)
uv run content/world/authoring/atlas.py build                               # outputs only, in artifacts/world-atlas/
uv run content/world/authoring/atlas.py seed-regions                        # append seeds for uncovered land
uv run content/world/authoring/atlas.py format                              # canonical JSON layout
blender --python content/world/authoring/atlas_blender.py -- import         # editable Blender scene
blender --background artifacts/world-atlas/continent.blend --python content/world/authoring/atlas_blender.py -- export
```

`uv` resolves the generator's two dependencies (numpy, Pillow) from the script header. Blender
5.2.2 LTS is optional, as for the Outpost layout bridge (`scripts/dev-blender.sh`).

## Constraints: established versus provisional

| Established (repository facts the atlas obeys) | Source |
| --- | --- |
| 1 m = 100 sim units, Y up, **−Z is north**, Greyhaven Outpost centre is (0, 0) | `docs/STARTER_ZONE.md`, `greyhaven_vale.rs` |
| Greyhaven zone 1 footprint: Vale x ±208, z ±208 m; Bandit Highlands north (z −632..−208, ~24–56 m above the Vale floor); Farmland and Hamlets south (z 208..640, +8 m); Marsh-Lake Wetlands east (x 208..632, +4 m) | `greyhaven_vale.rs`, Atlas 5.1.i424–i426 |
| The Vale is a sheltered crater-like valley with cliffs and terraces; a brook from the western rim leaves the Outpost south through the palisade | Atlas 5.1.84, 5.1.i513 |
| Established entrances: Highland Pass (north), Marsh Pass (east), the South Road notch (south); mountains beyond the walls are scenery | `STARTER_ZONE.md` |
| Harsher, dangerous highlands north; settled, gentler farmland south; wetlands east | Atlas 5.1.85 |
| Crossings between major regions are continuous physical passes without loading screens; authority boundaries use fenced seam corridors | Atlas 5.1.86, ADR 0007 |
| Fast travel only between hubs first reached on foot | Atlas 5.1.9 |
| Run speed 6.3 m/s (`RUN_SPEED_UNITS_PER_SECOND = 630`) | `mmorpg-core` |
| Owner direction (2026-11-10): continent about 20 × 20 km; regions about 2–4 km², about 150 of them | this task |

Everything else here is **provisional**: the Crown's shape and the meaning of its name, the
gates, kingdom names, borders and capitals, rivers, climate and every name not listed above.
The repository records no prior lore for the Shattered Crown or the kingdoms; the
interpretation below (Greyhaven's three passes are the established southern approaches to
the Crown) is the atlas's assumption, flagged as Question 1.

## Scale

The domain is 24 × 24 km on a 40 m grid with Greyhaven's own frame, so atlas metres are zone
metres. The continent covers about 349 km² of land, roughly 20 km across in both directions:
a coast-to-coast walk is about 55 minutes at running speed, comparable to a large MMO
continent. Elevations are compressed for play (the Crown's crest is about 1,150 m), so the
climate model uses an exaggerated 10 °C/km lapse rate to put snow on it.

## Arrangements considered

| Arrangement | Coherence | Play | Fit with Greyhaven | Verdict |
| --- | --- | --- | --- | --- |
| **A. Ring massif at the centre**: the Crown is an eroded domed caldera, its crest broken by five gorges ("gates"); kingdoms radiate around it, rivers drain radially | Domes with radial drainage and breached calderas are well attested; one wind gives a wet west and a rain-shadowed east | Every kingdom borders the contested heartland through its own gate; difficulty rises toward the centre; trade circles the ring | Greyhaven sits on the southern apron below the Greyfang Gate: highlands rise north into the Crown, farmland falls south | **Selected** |
| B. Spine cordillera dividing west from east | Strong orographic contrast | Two coasts, few chokepoint passes; a long linear world | Fits only if Greyhaven is a pass basin in the spine | Kept as alternative |
| C. Kingdoms around an inland sea, the Crown a broken northern arc | Plausible | Sea travel dominates; land routes are thin | Greyhaven would be a northern backwater | Kept as alternative |
| D. Crown peninsula: mountains in one corner, lowland kingdoms elsewhere | Plausible | The Crown is a cul-de-sac; kingdoms compete mainly with each other | Greyhaven at the edge of play | Rejected |

A fits the established Greyhaven facts best and gives the clearest gameplay structure:
levelling outward from Greyhaven and inward toward the Crown, five gates as contested
chokepoints, and a natural reason for five rival powers to meet. The alternatives can be
rebuilt from the same layers by moving the `crown` and `ridges` controls.

## The selected geography

**The Shattered Crown.** A domed uplift about 14 km across with a 2.7 km-radius crest ring
(about 1,150 m) around a crater basin, Crownhollow, holding Crownmere. Five gorges break
the ring, one facing each kingdom: the Greyfang Gate (south, toward Greyhaven), the Sundering
(north, where Crownmere drains out as the Isen), Vessgate (west), Sarr Gap (east) and Merrow
Notch (south-east). The interior is unclaimed, high-level ground with the ruins of Shardfall.

**Greyhaven.** The Vale is read as a **karst polje**: a closed, flat-floored basin in a
limestone apron with steep walls, drained underground. The atlas sends the Outpost's brook into
a ponor (Greyhaven Sink) just south of the palisade, and it resurfaces 1.5 km south at the
Greywell, the source of the Greywater. This explains the basin's lack of a surface outlet without
touching zone content. The three established passes join the atlas as follows:

- The Highland Pass leads north up the apron to the Greyfang Gate.
- The Farmland Road continues south to Millford and the King's Road.
- The Marsh Trail leads east to the Fenwater, which drains the Marsh-Lake Wetlands toward the Merrow delta.

![Greyhaven in its surroundings](images/world/greyhaven-context.png)

**Water.** Six authored rivers have strictly falling beds and meander deterministically: the
Greywater, Fenwater, Brannock (a Fenwater tributary from Merrow Notch), Vess, Isen and Sarr.
Everything else is derived: flow routing on a depression-filled surface, natural lakes where
depressions fill, and two declared sinks. One is the Saltmere, an endorheic salt pan in
Sarrath's rain shadow behind the coastal range of the Teeth; the other is the Greyhaven ponor.

![Hydrology](images/world/hydrology.png)

**Climate and vegetation.** Westerlies with a weaker southern sea wind. Rain falls on the
windward west (temperate rainforest) and on the Crown's west flank; the east lies in a rain
shadow (steppe, dry scrub and the salt pan). The cold north is conifer forest, the Crown alpine
and snow, and the centre and south temperate woodland and meadow. Greyhaven sits among montane
conifer and mixed woods, consistent with Wolfrun Woods, Hollowpine Reach and the highland pines.

![Biomes](images/world/biomes.png)

**Kingdoms (provisional).** Kingdoms claim whole regions by terrain-weighted travel cost from
their capital and towns; the Crown is unclaimed, and border regions within 18 % of a rival's
cost are marked contested.

| Kingdom | Character | Capital | Gate | Regions, area |
| --- | --- | --- | --- | --- |
| Carrowmark | Grain and river plain of the south; Greyhaven is its northern march fort | Carrow | Greyfang Gate | 44, 100 km² |
| Vesmere | Wet west coast: timber, ships, fisheries | Vesport | Vessgate | 30, 69 km² |
| Isenholt | Cold northern fells and firths: iron, furs | Isenhold | The Sundering | 12, 28 km² |
| Sarrath | Rain-shadow steppe: horses, salt, oasis towns | Sarrath | Sarr Gap | 30, 69 km² |
| Merrow League | Delta merchant towns holding the Fenwater and the sea lanes | Merrowgate | Merrow Notch | 19, 50 km² |

**Settlement and trade geography.** Capitals stand at river mouths or oases; towns at fords
and route junctions (Millford, Brannock Ford, Thornwick, Hagstead, Ashwell); forts at
approaches (Notchwatch). Eleven roads are routed over the terrain between them, crossing large
rivers only where they must. Every gate is reached by a road from its kingdom that continues to
Shardfall in Crownhollow, and the Salt Road links Isenholt, Sarrath and the Merrow League across
the dry east.

## Natural regions

Regions are the unit of play and claims, sized at about 2–4 km² (149 regions, median 2.3 km²).
Each grows from an editable seed by terrain-weighted travel cost, so borders fall on ridges,
large rivers and steep ground instead of straight lines. Neighbouring regions **overlap**: within
200 m of a border each point carries a secondary region and blend weight, so presentation can
compose continuous transitions. The 1,024 m streaming chunks are internal only: a check
evaluates the composition chunk by chunk and confirms it is identical to the whole-world result.
Greyhaven's region contains the whole zone footprint (4.8 km²), leaving room for the threefold
expansion already decided (Atlas 5.1.22).

![Regions](images/world/regions.png) ![Composition](images/world/composition.png)

The per-region adjacency table (kingdom, area, elevation, biomes, and each neighbour with its
border length and crossing kind: open, ridge, pass or river) is generated as
`artifacts/world-atlas/adjacency.md` and `atlas-report.json`.

**Macro adjacency plan.**

| From | To | Natural crossing |
| --- | --- | --- |
| Greyhaven | Crownhollow | Highland Pass, then the Greyfang Gate gorge |
| Greyhaven | Carrowmark heartland | Farmland Road, then the Greywater valley (King's Road) |
| Greyhaven | Merrow League | Marsh Trail, then down the Fenwater (Fen Road) |
| Carrowmark | Vesmere | Gap between Westspur and the Vess Downs (West Road) |
| Carrowmark | Merrow League | Fenward Ridge and the South Coast Road |
| Vesmere | Isenholt | Coastal lowland west of the Isen Fells (North Road) |
| Isenholt | Sarrath | North coast (Salt Road) |
| Sarrath | Merrow League | Dry plains west of the Teeth (Salt Road via Ashwell) |
| Each kingdom | Crownhollow | Its own gate gorge |

## Authoring layers

All sources are JSON in `content/world/`, applied in a fixed order. Each is hand-editable,
and the generator never writes any of them.

| Order | Layer | Holds | Edited by |
| --- | --- | --- | --- |
| 1 | `terrain.json` | Coast control polygon, seas, islands, the Crown, ridges, basins, noise | Hand or Blender |
| 2 | `water.json` | Major rivers, lakes to expect, sinks, springs, carving | Hand or Blender |
| 3 | `overrides/height.json` | Sparse elevation deltas per atlas cell | Hand |
| 4 | `anchors.json` | Hand-authored places the atlas fits around (Greyhaven), with stand-in elevations and entrances | Hand |
| 5 | `climate.json` | Wind, temperature, rain model, biome rules | Hand |
| 6 | `overrides/biome.json` | Sparse biome overrides per atlas cell | Hand |
| 7 | `regions.json` | Region seeds; ids are stable | Hand, Blender, or `seed-regions`, which only appends |
| 8 | `polities.json` | Kingdoms, pinned claims, settlements, routes | Hand or Blender |

The preservation rules:

- **Anchors win over everything.** The checks confirm the zone footprint keeps its stand-in
  elevations, gets no generated lake or river, and joins its surroundings smoothly.
- **Overrides are separate files** in the existing paint convention (`[column, row, value]`), so
  regenerating never loses them.
- **Region ids never change.** The Blender export writes back only moved geometry and leaves an
  untouched file byte-identical.
- **The build checks its own side effects.** It verifies that `content/world/` and
  `content/greyhaven/` are unchanged by it.

## Integration with existing authoring

Inside an anchor the atlas never generates content. Greyhaven's terrain, landscape JSON, paint
layers, Blender-authored collision and quests remain the authority, and their content revision
is untouched. The atlas reuses the existing conventions instead of adding a parallel system:

- the zone frame and units;
- the sparse paint-cell format;
- the Blender bridge pattern of `scripts/dev-blender.sh`;
- the `[x, z, …]` polylines of `landscape.json`.

When a new zone is opened (see the plan), its atlas window becomes the *starting* macro terrain
and is then detailed through the same tools as Greyhaven: `landscape.json` landforms, World
Studio's Krita paint, and Blender collision meshes. The atlas then records that zone as a new
anchor, and from then on the zone's content is the authority there.

## Consistency checks

`atlas.py check` fails on any violation:

- **Coast and scale:** one continent plus the declared islands, within the target land area.
- **Rivers:** every bed falls strictly, reaches its declared mouth (sea, sink or parent river),
  and has derived drainage along at least 85 % of its course.
- **Water:** every land cell drains to the sea or a declared sink; every flow step descends; the
  karst resurgence lies below its ponor; Crownmere forms.
- **Anchors:** stand-in elevations are kept, the footprint stays dry, and the surroundings stay
  continuous.
- **Regions:** they cover all land, are contiguous, and come to about 150 in the 2–4 km² band;
  per-chunk composition equals the whole.
- **Kingdoms:** each contains its capital, has at least eight regions and borders the Crown;
  every road leg routes over land.
- **Reproducibility and preservation:** two builds give the same fingerprint, and neither source
  tree changes during a build.

## Questions for the owner

1. **The Shattered Crown's established entrances.** The repository has no lore for the Crown or
   for the kingdoms. The atlas treats Greyhaven's Highland, Marsh and South passes as the
   established southern approaches and adds five provisional gates. If the Crown or its
   entrances were defined elsewhere, which definition should replace this one?
2. **Arrangement.** Keep the central ring massif (A), or prefer the spine (B) or the inland sea (C)?
3. **Kingdoms.** Are five kingdoms right? Should Greyhaven belong to Carrowmark, or stand as a
   neutral march between Carrowmark and the Merrow League?
4. **Scale.** Is Greyhaven's whole current zone (about 1.1 km² walkable, inside a 4.8 km² region)
   the intended density for a region, or should regions be denser?

## Implementation plan (prioritized)

1. **Owner review of this atlas.** Answer the questions above, then record the accepted parts as
   Decision Atlas entries under an issue.
2. **Greyhaven horizon.** Feed the atlas's far terrain around the zone into `mmorpg-scenery`'s
   presentation-only distant mountains, replacing the generic ring with the actual Crown, apron
   and valleys. No collision or content-revision change.
3. **Zone template from an atlas window.** A World Studio export that turns an atlas region (or
   group of regions) into the starting `landscape.json`, paint and height grid of a new zone,
   then registers it as an anchor. This is the first real use of the atlas in authority content,
   and it needs its own content revision and owner approval.
4. **Seam corridors on region borders.** Choose logical-zone boundaries along atlas region
   borders at passes and fords, using the existing ADR 0007 corridors.
5. **Hub and waystone network.** Place exploration-unlocked hubs (Atlas 5.1.9) on the atlas
   settlements, and size the routes for walking times.
6. **Region content briefs.** Generate a one-page brief per region from the atlas report (biome,
   neighbours, kingdom, level band) for later detailing; name regions as they are authored.
7. **Refinement.** Raise the atlas resolution near planned zones, add tributaries and coastal
   features, and review climate parameters against art direction.

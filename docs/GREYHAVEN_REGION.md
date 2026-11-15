# The Greyhaven region

The Greyhaven zone covers the continent atlas's whole Greyhaven Vale region: about 3.7 km ×
2.6 km (4.9 km²) inside its boundary wall (#810, #812). This document is its design (#816,
Atlas 5.1.i816): one landscape instead of rectangles glued together.

![The redesigned region from above](images/world/greyhaven-redesign.jpg)

## Why it was redesigned

The region grew in increments. The Vale, then the Farmland, Highlands and Wetlands, each a
rectangle with a straight ridge against the next, then the atlas's new ground round them. It read
as separate boxes:

- straight ridges and walls between the regions;
- grid roads, rectangular fields and forests;
- flat, empty ground;
- isolated grass tufts;
- paths that clipped through the ground.

## The geography

The Vale lies in a basin under the Crown mountains. The land rises north to the mountains and
falls away west and south. Two rivers drain it:

- **Greywater:** the village brook leaves the Vale south through the farmland and the King's Road
  Downs.
- **Fenwater:** the Marsh Lake drains east past a ford at Fenwater Crossing.

Mountain streams feed both basins:

- the **Delving Run** comes down a ravine from the Delving Heights into the Marsh Lake;
- the **Westmere Beck** fills **Westmere Tarn**, which overflows west.

| Sub-region | Character | Creatures (levels) |
| --- | --- | --- |
| Greyhaven Vale | The authored heart round the Outpost's basin, and the land round it | the quest chain's |
| Bandit Highlands | Greyfang Ridge and the uplands beyond its pass, rising to the mountains | Redbrand garrison (3–5) |
| Farmland and Hamlets | Rolling downs with three hamlets, hedgerow trees and the landmark tower | — |
| Marsh-Lake Wetlands | The marsh basin round the lake and pools, alder copses | lurkers, boars, wolves |
| Greyfang Approach | Pine slopes and scree under the mountains, the road north | Redbrand outriders (4–5) |
| Delving Heights | Rugged high ground round the Delving Run's ravine | Redbrand veterans (5), hill ogres |
| Fenwater Crossing | Meadows and alders along the river, the ford | field marauders (3), boars |
| King's Road Downs | Open downs with copses and a spinney by the Greywater | marauders (3), boars, rats |
| Southwood | Mixed oak, birch and pine wood | boars (1–2), wolves (2) |
| Westmere Woods | Dense pine wood round the tarn | wolf packs (2), boars, tarn lurkers (3) |
| Crown Foothills | Pine and boulder slopes in the north-west | wolf packs (2), a hill ogre |

Difficulty rises away from the Vale, on the existing creature templates.

## Roads

Every sub-region is on a road, and loops join them without passing through the Outpost:

- **King's Road:** from the South Road through the hamlets and down the Greywater's west bank to
  the southern edge, toward Millford. The hamlets' lanes branch off it.
- **Greyfang Road:** from the Redbrand camp at the Greyfang cave's north mouth, winding up
  through the **Highland Pass** (a saddle in Greyfang Ridge), past the stronghold (by the
  **Stronghold Way**) and the **Lookout Path**, north to the mountains.
- **Delving Track:** east from the lookout across the ravine to the Delving Heights.
- **Fen Road:** east from the top of the Millbrook Climb, south of the Marsh Lake and down the
  Fenwater to
  its ford. The **Marsh Trail** leads off it to the causeway.
- **Fenwater Link:** from the Fen Road south to the King's Road.
- **Westmere Road:** west from the top of the Wolfrun Climb to the tarn. The **Crown Track** leads off it
  north into the Crown Foothills.
- **Southwood Trail** and **Westwood Track:** from the south hamlet through Southwood and back
  north to the Westmere Road.

Roads are graded into the ground (see below), so their surface is smooth and they never clip it.
Where a road meets a river it fords it.

## How it is built

Only the Vale's **heart** is hand-authored: the Outpost's basin and the rim round it with
Millbrook Farm, Stillwater Lake and the Sunken Shrine, Redbrand Hollow and the Greyfang cave.
Its outline is the `heart` in `design.json`, rounded into curves and swollen outwards by up to
15 m in broad lobes (`greyhaven_vale::beyond_core`), so no straight edge or square ring is left
of the old rectangular core. The old outer valley ring, its square Vale Ring Road, the straight
escarpment and its right-angled pass, and the farmland's and wetlands' ridges are gone.
Everything round the heart comes from committed data:

- **`content/greyhaven/design.json`** holds the design, in metres:
  - height controls and roughness;
  - the lakes and rivers;
  - the roads;
  - the forests (outline, species, spacing and cover);
  - the rock fields.
- **`content/greyhaven/authoring/region_design.py`** turns the controls, roughness, lakes and
  rivers into the macro terrain, **`content/greyhaven/terrain.json`** (16 m samples). It blends
  the controls over the atlas base terrain and adds three kinds of relief: rolling hills,
  walking-scale knolls and crags that grow with height. It also cuts each river's valley so the
  river never runs uphill. `--check` reports when the committed terrain is stale.
- **The zone** (`greyhaven_vale::design`, `zone_ground`) builds its 4 m ground from these layers:
  - the macro terrain;
  - the river channels, cut at the 4 m grid and filled with water reaches;
  - level pads under the yards, fields and buildings (buildings in a yard share its height);
  - the wetlands' own waters, island and causeway;
  - the roads, each graded to a smoothed profile along it except at fords; a branch eases onto
    the height of the road it leaves.

  Within 40 m of the heart this blends into the authored ground.
- **Forests and rock fields** are colliders: trunks and boulders, kept clear of roads, waters,
  pads, creature posts and each other. The clients draw the ground cover from the land itself:
  - forest floor under the closer forests;
  - damp hollows by the rivers and the marsh;
  - rock on cliffs, highland grass high up;
  - meadow in broad patches;
  - undergrowth clumps in and along the forests;
  - tussocks, flower drifts and scrub gathered in patches across the open land.

  The heart's own woods and groves are rounded into clumps. Beyond the wall the mountains are
  wooded low down, with crags, rock and snow above.

## Settlements and landmarks

The roads lead to places (#818, `greyhaven_vale::settlements`). Each is a named area inside its
sub-region (it names the ground it stands on), its yard is a level pad of the designed land, and
its buildings reuse the Vale's visuals:

| Place | Where | What stands there |
| --- | --- | --- |
| Barrowdown | A knoll of the King's Road Downs, off the Fenwater Link by Barrowdown Lane | Three houses, the Barrow Rest alehouse, a barn, a well, a waystone |
| Barrowdown Ring | West of the hamlet | Eight standing stones round an old barrow |
| Fenford | North of the Fenwater's ford, up Fenford Lane from the Fen Road | Three houses, the Ford Inn, a mill, a well, a waystone |
| Westmere Lodge | Westmere Tarn's eastern shore, at the Westmere Road's end | The fisher's lodge, a tent and fire, a waystone |
| Hunters' Camp | On the Southwood Trail | A lodge, tents, a fire and drying racks |
| Delving Workings | At the Delving Track's end | A mine mouth in the rock, a watchtower, a camp; the Redbrands hold it |
| Greyfang Watch | Beside the Greyfang Road below the mountains | A tall watchtower and a fire |
| Crown Stones | Beside the Crown Track | A ring of eight standing stones |
| Delving Run footbridge | Where the Delving Track crosses the Delving Run | A 9 m plank footbridge on trestles (#828), asset-tooling's `settlement.props.footbridge` (asset-tooling#651); players walk its deck, end ramps and between its rails |

Barrowdown, Fenford and Westmere Lodge hold the network's fourth to sixth waystones, so a
traveller who has found them can return there from the Outpost's stone.

## People and their days

Greyhaven's people keep a day (#819, `greyhaven_vale::residents`). Each works at its post by day,
walks home at dusk (20:00) and back at dawn (06:00), and runs errands at set hours: out along an
authored path, a while at the far end, then back to work (`mmorpg_core::Errand`).
Where a resident is is a pure function of the zone's tick, so it needs no saved state and replays
exactly.

- **The Outpost's residents** live in the houses round the square. The weaver, the fisher and the
  baker fetch water at the well, Davin and the peddler call at the inn, the porter waits for loads
  at the north and east gates, the washer works at the brook below the bridge, the peddler greets
  travellers at the south gate, the herbalist gathers along the South Road each morning and the
  miller spends his working day at Millbrook Farm's windmill.
- **Barrowdown:** Gwen Barrow keeps the Barrow Rest, Shepherd Corin and Farmer Tobin Hale take
  their midday ale there, Old Maud keeps watch at the barrow ring, and Pip runs to the stones in
  the morning and to the alehouse in the afternoon.
- **Fenford:** Hob Ferris keeps the Ford Inn, Miller Wynne works the mill, Ferryman Osk waits at
  the ford, Netter Ilsa mends nets in the square, and Trader Maren keeps a stall with the
  region's gear by day (#826): padded armour, the buckler and shortsword, and the level 6 mace,
  jerkin and cowl.
- **Westmere Lodge:** Fisher Brann fishes the tarn and eats at the fire at noon.
- **The hunters' camp:** Huntsman Edric Vale keeps the fire; Trapper Sela walks her lines in
  Southwood each morning.
- **Greyfang Watch:** Watchman Garth holds the tower, Watchman Pell walks the road north twice a
  day.
- **The farmland's hamlets:** Goodwife Ena, Ploughman Garr, Old Bertil, Shepherd Lise, Miller
  Hanne and Farmhand Jory work their fields, wells and mills.

## Quests

Sixteen quests carry players from the Vale's chain out into the region (#820,
`greyhaven_vale::quests`). The Marshal starts three chains, each through one or two
settlements:

| Chain | Starts | Giver | Quests |
| --- | --- | --- | --- |
| The Downs and the ford | After *Trouble in the Woods* | Gwen Barrow at Barrowdown, then Hob Ferris at Fenford | *Word from the Downs*, *Raiders on the Downs*, *Grain for the Barrow Rest* (stolen grain sacks from marauders), *The Barrow Ring* (hear out Old Maud), *Down to the Ford*, *Tusks and Tallow* (boar tusks), *The Fenwater Raiders* |
| The tarn and Southwood | After *Scout the Lake* | Brann at Westmere Lodge, then Edric Vale at the hunters' camp | *The Lodge on the Tarn*, *Scales of Westmere* (lurker scales), *Wolves at the Water*, *The Hunters' Camp*, *Southwood Hunt* |
| The Greyfang Road | After *Garrick Redbrand* | Watchman Garth at Greyfang Watch | *The Greyfang Watch*, *Orders on the Road* (crossbowmen's orders), *The Delving Workings* (retake the mine camp), *Ogres of the Heights* |

The four quest items (boar tusks, grain sacks, lurker scales and Redbrand orders) drop only while a
quest needs them. Rewards are experience, copper and the Vale's existing gear.

Review the Downs chain with `?scenario=greyhaven-downs`: a level 4 Warden on Barrowdown Lane with
*Trouble in the Woods* turned in and *Word from the Downs* accepted, a few steps from Gwen Barrow.

## Grass and houses

Grass grows as blades, not tufts (#832). `mmorpg_presentation::grass` decides where: thickly on
meadow and foothills, thinner on the hub, hollows and highland, sparse under woods, and nowhere the
near terrain's overlays (forest floor, gravel, mud, sand, paths, rock) or its worn roads, plaza and
fields cover the turf, nor in water, river beds, terrain holes or the footprint of anything that
stands on the ground. Each clump takes the ground's tone, leaning greener, with dry straw patches
over about 18 m. It lays out 16 m cells on a 0.3 m jittered lattice; a cell's clumps come in a
random order, so any prefix is an even thinning. The browser draws every clump of a cell within
12 m of the eye and thins them out to 48 m, where the field sinks away; each clump is 15 thin,
tapered, curved blades, bent by the wind and pushed aside round the player's feet in the vertex
stage. The old procedural tufts draw nothing; the native client has no grass field yet.

The region's seven 8 × 6 m houses are the Outpost's timber-framed house in asset-tooling's Outpost
kit, each door turned to its settlement's yard (`content/greyhaven/dressing/settlements.json`,
written by `cargo run -p mmorpg-scenery --example export-settlement-dressing`); their boxes remain
the stand-in until the kit loads. Westmere Lodge's larger hall, the farmhouses, barns and the inn
keep the Vale's procedural buildings.

## Not yet done

- **Homes you can enter:** residents end their day on their doorstep; only the Marshal and the
  tanner sleep indoors.
- **The basin's climbs' ground:** the six ways up the heart's 11 m cliffs are still straight ledges
  cut into the cliff; only their roads round their turns (every authored road turns on a 4 m curve).
- **The forest-floor texture:** its overlay repeats every 3 m and reads as stripes on open ground,
  so the designed forests show the woods' darker ground tone instead; the heart's woods keep it.
- **Visual acceptance** from the owner.

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
| Greyhaven Vale | The authored basin with the Outpost, unchanged | the quest chain's |
| Bandit Highlands | Uplands above the authored escarpment, rising to the mountains | Redbrand garrison (3–5) |
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
- **Greyfang Road:** from the top of the Highland Pass, past the stronghold (by the **Stronghold
  Way**) and the **Lookout Path**, north to the mountains.
- **Delving Track:** east from the lookout across the ravine to the Delving Heights.
- **Fen Road:** east from the Vale Ring Road, south of the Marsh Lake and down the Fenwater to
  its ford. The **Marsh Trail** leads off it to the causeway.
- **Fenwater Link:** from the Fen Road south to the King's Road.
- **Westmere Road:** west from the Downs Trail to the tarn. The **Crown Track** leads off it
  north into the Crown Foothills.
- **Southwood Trail** and **Westwood Track:** from the south hamlet through Southwood and back
  north to the Westmere Road.

Roads are graded into the ground (see below), so their surface is smooth and they never clip it.
Where a road meets a river it fords it.

## How it is built

The hand-authored **core** stays as it was: the Vale and the highlands' escarpment above it
(`greyhaven_vale::CORE`). Everything round it comes from committed data:

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
  - the roads, each graded to a smoothed profile along it except at fords.

  Within 60 m of the core this blends into the authored ground.
- **Forests and rock fields** are colliders: trunks and boulders, kept clear of roads, waters,
  pads, creature posts and each other. The clients draw the ground cover from the land itself:
  - forest floor under the closer forests;
  - damp hollows by the rivers and the marsh;
  - rock on cliffs, highland grass high up;
  - meadow in broad patches;
  - undergrowth clumps in and along the forests.

  Beyond the wall the mountains are wooded low down, with rock and snow above.

## Not yet done

- **Landmarks** in the new sub-regions: the Delving workings with their bridge over the ravine, a
  Greyfang watchtower, hamlets on the downs.
- **Quests** and NPCs out there.
- **The core's own rectangles:** the square Vale Ring Road and the Vale's woods.
- **Visual acceptance** from the owner.

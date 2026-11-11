# /// script
# requires-python = ">=3.11"
# dependencies = ["shapely>=2"]
# ///
"""GIS exchange test, on a copy of content/world/: the region topology is valid and shares its
arcs, an untouched export and import changes nothing, an outdated folder is refused, and a
junction move and a region split made in the WGS84 files come back and pass the atlas checks.

    uv run content/world/authoring/test_atlas_gis.py
"""

import json
import math
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from shapely import is_ccw
from shapely.geometry import Polygon

HERE = Path(__file__).resolve().parent
SOURCES = HERE.parent
work = Path(tempfile.mkdtemp(prefix="atlas-gis-"))
world = work / "world"
shutil.copytree(SOURCES, world, ignore=shutil.ignore_patterns("authoring"))
failures = []


def atlas(*args, ok=True):
    run = subprocess.run(["uv", "run", str(HERE / "atlas.py"), *args], env={**os.environ, "ATLAS_WORLD": str(world)},
                         capture_output=True, text=True)
    if ok and run.returncode:
        raise SystemExit(f"atlas.py {' '.join(args)} failed:\n{run.stdout[-1500:]}\n{run.stderr[-1500:]}")
    return run


def expect(ok, message):
    print(("ok   " if ok else "FAIL ") + message)
    if not ok:
        failures.append(message)


def snapshot():
    return {p.name: p.read_bytes() for p in sorted(world.glob("*.json"))}


def load(path):
    return json.loads(Path(path).read_text())


def save(path, data):
    Path(path).write_text(json.dumps(data))


try:
    gis = work / "gis"
    atlas("gis-export", "--out", str(gis))
    topo = load(gis / "epsg3857" / "regions.topojson")
    arcs = topo["arcs"]
    uses = [0] * len(arcs)
    polygons, bad = {}, []
    for geom in topo["objects"]["regions"]["geometries"]:
        rings = []
        for ring in geom["arcs"]:
            pts = []
            for ref in ring:
                uses[ref if ref >= 0 else ~ref] += 1
                seg = arcs[ref] if ref >= 0 else arcs[~ref][::-1]
                pts += seg if not pts else seg[1:]
            rings.append(pts)
        p = Polygon(rings[0], rings[1:])
        polygons[geom["id"]] = p
        if not p.is_valid or not is_ccw(p.exterior):
            bad.append(geom["id"])
    coast = uses.count(1)
    expect(not bad and set(uses) <= {1, 2},
           f"{len(polygons)} region polygons are valid with counter-clockwise exteriors {bad or ''}; each of {len(arcs)} arcs "
           f"is used once (coast, {coast}) or twice (shared by two regions, {uses.count(2)})")
    land = sum(p.area for p in polygons.values()) / 1e6
    expect(330 < land < 360, f"the regions cover {land:.0f} km² of land")

    before = snapshot()
    run = atlas("gis-import", str(gis / "wgs84"))
    expect(snapshot() == before, "an untouched export and import leave every source byte-identical")

    # move a junction in the WGS84 files, as a GIS tool would save it
    d = gis / "wgs84"
    junctions = load(d / "junctions.geojson")
    j = junctions["features"][100]
    jid = j["properties"]["id"]
    j["geometry"]["coordinates"][0] += 60 / 111319.49  # 60 m east
    save(d / "junctions.geojson", junctions)
    atlas("gis-import", str(d))
    borders = load(world / "borders.json")
    at = next(x["at"] for x in borders["junctions"] if x["id"] == jid)
    ends = [bd["points"][0] if bd.get("from") == jid else bd["points"][-1] for bd in borders["borders"] if jid in (bd.get("from"), bd.get("to"))]
    old = next(f for f in load(gis / "epsg3857" / "junctions.geojson")["features"] if f["properties"]["id"] == jid)["geometry"]["coordinates"]
    expect(at == [round(old[0]) + 60, -round(old[1])] and all(e == at for e in ends),
           f"moving junction {jid} 60 m east in WGS84 moves it and the ends of its {len(ends)} borders")
    stale = atlas("gis-import", str(d), ok=False)
    expect(stale.returncode != 0 and snapshot() != before, "importing the same folder again is refused: the sources changed since it was exported")

    # split a region: a new line across it and a new seed, both without ids
    atlas("gis-export", "--out", str(gis))
    sides = {f["properties"]["id"]: f["geometry"]["coordinates"] for f in load(d / "borders.geojson")["features"]}
    region = "r113"
    a, b = sides["b294"][len(sides["b294"]) // 2], sides["b325"][len(sides["b325"]) // 2]
    line = [[a[0] + (b[0] - a[0]) * t / 6, a[1] + (b[1] - a[1]) * t / 6] for t in range(7)]
    seed = next(f for f in load(d / "seeds.geojson")["features"] if f["properties"]["id"] == region)["geometry"]["coordinates"]
    nx, ny = -(b[1] - a[1]), b[0] - a[0]
    side = -1 if (seed[0] - a[0]) * nx + (seed[1] - a[1]) * ny > 0 else 1
    n = math.hypot(nx, ny)
    new_seed = [(a[0] + b[0]) / 2 + side * nx / n * 450 / 111319.49, (a[1] + b[1]) / 2 + side * ny / n * 450 / 111319.49]
    fc = load(d / "borders.geojson")
    fc["features"].append({"type": "Feature", "properties": {}, "geometry": {"type": "LineString", "coordinates": line}})
    save(d / "borders.geojson", fc)
    fc = load(d / "seeds.geojson")
    fc["features"].append({"type": "Feature", "properties": {}, "geometry": {"type": "Point", "coordinates": new_seed}})
    save(d / "seeds.geojson", fc)
    out = atlas("gis-import", str(d)).stdout
    regions = [r["id"] for r in load(world / "regions.json")["regions"]]
    check = atlas("build", "--checks", "--out", str(work / "out"), ok=False)
    failed = [line for line in check.stdout.splitlines() if line.startswith("FAIL")]
    expect("new regions" in out and check.returncode == 0,
           f"a line and a seed drawn in WGS84 split {region} into {region} and {regions[-1]}, and the atlas checks pass {failed or ''}")
finally:
    shutil.rmtree(work)
sys.exit(1 if failures else 0)

# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy>=2", "pillow>=10.1"]
# ///
"""Greyhaven region terrain from its design (#816).

    uv run content/greyhaven/authoring/region_design.py            # write terrain.json and previews
    uv run content/greyhaven/authoring/region_design.py --check    # fail when terrain.json is stale

Reads the atlas base terrain and outline from content/greyhaven/region.json and the hand-authored
design from content/greyhaven/design.json (height controls, roughness, lakes and rivers), and writes
the macro terrain the zone samples beyond its hand-authored core to content/greyhaven/terrain.json:
heights in units (cm) on a 16 m grid. The zone carves the river channels, grades the roads and
levels the pads under structures itself, at its own 4 m grid. Previews go to
artifacts/greyhaven-region/.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

REPO = Path(__file__).resolve().parents[3]
GREYHAVEN = REPO / "content" / "greyhaven"
OUT = REPO / "artifacts" / "greyhaven-region"
SEED = 0x4752_4559


def load(name: str):
    return json.loads((GREYHAVEN / name).read_text())


def grid(design):
    g = design["grid"]
    xs = g["origin"][0] + np.arange(g["columns"]) * g["step"]
    zs = g["origin"][1] + np.arange(g["rows"]) * g["step"]
    return np.meshgrid(xs.astype(float), zs.astype(float))


def atlas_base(region, X, Z):
    """The atlas elevation (metres), bilinear between its samples and clamped at its edge."""
    t = region["terrain"]
    H = np.array(t["heights"], float) / 100.0
    c = np.clip((X * 100 - t["origin"][0]) / t["step"], 0, t["columns"] - 1.001)
    r = np.clip((Z * 100 - t["origin"][1]) / t["step"], 0, t["rows"] - 1.001)
    c0, r0 = c.astype(int), r.astype(int)
    fc, fr = c - c0, r - r0
    top = H[r0, c0] * (1 - fc) + H[r0, c0 + 1] * fc
    bottom = H[r0 + 1, c0] * (1 - fc) + H[r0 + 1, c0 + 1] * fc
    return top * (1 - fr) + bottom * fr


def blend(points, X, Z, key):
    """Normalised Gaussian blend of point values, and how much the points cover (0..1)."""
    num = np.zeros_like(X)
    den = np.zeros_like(X)
    miss = np.ones_like(X)
    for p in points:
        d2 = (X - p["at"][0]) ** 2 + (Z - p["at"][1]) ** 2
        w = np.exp(-d2 / p["radius"] ** 2)
        num += w * p[key]
        den += w
        miss *= 1 - w
    return num / np.maximum(den, 1e-9), 1 - miss


def lattice(ix, iz, seed):
    h = (ix.astype(np.int64) * 374_761_393 + iz.astype(np.int64) * 668_265_263 + seed) & 0xFFFFFFFF
    h = ((h ^ (h >> 13)) * 1_274_126_177) & 0xFFFFFFFF
    return ((h ^ (h >> 16)) & 0xFFFF) / 65535.0 * 2 - 1


def value_noise(X, Z, wavelength, seed):
    x, z = X / wavelength, Z / wavelength
    ix, iz = np.floor(x), np.floor(z)
    fx, fz = x - ix, z - iz
    sx, sz = fx * fx * (3 - 2 * fx), fz * fz * (3 - 2 * fz)
    a, b = lattice(ix, iz, seed), lattice(ix + 1, iz, seed)
    c, d = lattice(ix, iz + 1, seed), lattice(ix + 1, iz + 1, seed)
    return (a * (1 - sx) + b * sx) * (1 - sz) + (c * (1 - sx) + d * sx) * sz


def fbm(X, Z, wavelength, octaves, seed):
    total, amp, norm = np.zeros_like(X), 1.0, 0.0
    for o in range(octaves):
        total += amp * value_noise(X, Z, wavelength / 2**o, seed + o * 7919)
        norm += amp
        amp *= 0.5
    return total / norm


def polyline_distance(X, Z, pts):
    """Distance to a polyline and the distance along it of the nearest point."""
    best = np.full(X.shape, np.inf)
    along = np.zeros_like(X)
    run = 0.0
    for (ax, az), (bx, bz) in zip(pts, pts[1:]):
        vx, vz = bx - ax, bz - az
        length = math.hypot(vx, vz)
        t = np.clip(((X - ax) * vx + (Z - az) * vz) / (length * length), 0, 1)
        d = np.hypot(X - (ax + t * vx), Z - (az + t * vz))
        closer = d < best
        best = np.where(closer, d, best)
        along = np.where(closer, run + t * length, along)
        run += length
    return best, along


def smoothstep(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0), 0, 1)
    return t * t * (3 - 2 * t)


def carve_rivers(H, X, Z, design, step):
    """Each river runs downhill: its bed follows the ground along it, never rising, `depth`
    below it, and the ground within its valley falls towards it."""
    for river in design["rivers"]:
        pts = river["points"]
        d, along = polyline_distance(X, Z, pts)
        # the bed profile: ground sampled along the line, its running minimum, less the depth
        samples = []
        run = 0.0
        for (ax, az), (bx, bz) in zip(pts, pts[1:]):
            n = max(2, int(math.hypot(bx - ax, bz - az) / step))
            for k in range(n):
                t = k / n
                samples.append((run + t * math.hypot(bx - ax, bz - az), ax + t * (bx - ax), az + t * (bz - az)))
            run += math.hypot(bx - ax, bz - az)
        samples.append((run, *pts[-1]))
        s_along = np.array([s[0] for s in samples])
        cols = np.clip(((np.array([s[1] for s in samples]) - X[0, 0]) / step).round().astype(int), 0, X.shape[1] - 1)
        rows = np.clip(((np.array([s[2] for s in samples]) - Z[0, 0]) / step).round().astype(int), 0, X.shape[0] - 1)
        ground = H[rows, cols]
        bed = np.minimum.accumulate(ground) - river["depth"]
        bed_at = np.interp(along, s_along, bed)
        half, valley = river["width"] / 2, river["valley"]
        bank = bed_at + 0.8 + (d - half) * 0.08
        shaped = np.where(d <= half, bed_at, np.minimum(H, bank))
        weight = 1 - smoothstep(half, valley, d)
        H = H * (1 - weight) + shaped * weight
    return H


def basin_lakes(H, X, Z, design):
    for lake in design["lakes"]:
        (cx, cz), (rx, rz) = lake["centre"], lake["radii"]
        e = np.hypot((X - cx) / rx, (Z - cz) / rz)
        bowl = lake["surface"] - lake["depth"] * np.clip(1 - e, 0, 1) ** 0.7
        shore = lake["surface"] + 0.4 + np.maximum(e - 1, 0) * 25
        shaped = np.where(e <= 1, bowl, np.minimum(H, shore))
        weight = 1 - smoothstep(1.0, 1.8, e)
        H = H * (1 - weight) + shaped * weight
    return H


def blur(H, passes=1):
    for _ in range(passes):
        P = np.pad(H, 1, mode="edge")
        H = (P[:-2, 1:-1] + P[2:, 1:-1] + P[1:-1, :-2] + P[1:-1, 2:] + 4 * H) / 8
    return H


def build():
    design, region = load("design.json"), load("region.json")
    X, Z = grid(design)
    step = design["grid"]["step"]
    base = atlas_base(region, X, Z)
    target, cover = blend(design["controls"], X, Z, "height")
    H = base * (1 - cover) + target * cover
    amplitude, _ = blend(design["roughness"], X, Z, "amplitude")
    hills = fbm(X, Z, 420, 4, SEED)
    ridges = 1 - np.abs(fbm(X, Z, 260, 3, SEED + 101))
    # Rolling ground at walking scale: knolls and dips over about 110 m.
    knolls = fbm(X, Z, 110, 3, SEED + 211)
    H = H + amplitude * (0.75 * hills + 0.5 * (ridges - 0.5) + 0.45 * knolls)
    # The mountains grow craggier with height: ridged spurs and gullies above 80 m.
    crags = 1 - np.abs(fbm(X, Z, 150, 4, SEED + 307))
    H = H + np.clip((H - 80) / 220, 0, 1) * 34 * (crags - 0.45)
    H = blur(H, 2)
    H = carve_rivers(H, X, Z, design, step)
    H = basin_lakes(H, X, Z, design)
    return design, region, X, Z, H


def terrain_doc(design, H):
    g = design["grid"]
    return {"version": 1,
            "note": "Generated by content/greyhaven/authoring/region_design.py from design.json and region.json; do not edit. "
                    "Heights in units (cm) relative to the Vale's floor on the grid below (units).",
            "origin": [g["origin"][0] * 100, g["origin"][1] * 100], "step": g["step"] * 100,
            "columns": g["columns"], "rows": g["rows"],
            "heights": np.rint(H * 100).astype(int).tolist()}


def text(doc) -> str:
    rows = ",\n    ".join(json.dumps(row, separators=(",", ":")) for row in doc["heights"])
    head = {k: v for k, v in doc.items() if k != "heights"}
    body = json.dumps(head, indent=2)[:-2]
    return body + ',\n  "heights": [\n    ' + rows + "\n  ]\n}\n"


def preview(design, region, X, Z, H, path):
    step = design["grid"]["step"]
    gz, gx = np.gradient(H * 2.0, step)
    shade = np.clip(0.62 + (-gx - gz) * 0.45, 0.15, 1.25)
    t = np.clip((H + 60) / 260, 0, 1)
    low, mid, high = np.array([0.36, 0.52, 0.30]), np.array([0.55, 0.58, 0.38]), np.array([0.62, 0.6, 0.58])
    rgb = np.where(t[..., None] < 0.5, low + (mid - low) * (t[..., None] * 2), mid + (high - mid) * (t[..., None] * 2 - 1))
    rgb = np.clip(rgb * shade[..., None], 0, 1)
    S = 4
    img = Image.fromarray((rgb * 255).astype(np.uint8)).resize((X.shape[1] * S, X.shape[0] * S), Image.BICUBIC)
    dr = ImageDraw.Draw(img)
    ox, oz = X[0, 0], Z[0, 0]

    def px(x, z):
        return ((x - ox) / step * S, (z - oz) / step * S)

    for lake in design["lakes"]:
        (cx, cz), (rx, rz) = lake["centre"], lake["radii"]
        dr.ellipse([px(cx - rx, cz - rz), px(cx + rx, cz + rz)], fill=(70, 120, 170))
    for river in design["rivers"]:
        dr.line([px(*p) for p in river["points"]], fill=(70, 120, 190), width=max(2, int(river["width"] / step * S)))
    for road in design.get("roads", []):
        dr.line([px(*p) for p in road["points"]], fill=(160, 120, 80), width=3)
    outline = [(x / 100, z / 100) for x, z in region["outline"]]
    dr.line([px(*p) for p in outline + outline[:1]], fill=(220, 60, 50), width=3)
    dr.rectangle([px(-208, -208), px(208, 208)], outline=(255, 230, 120), width=2)
    for c in design["controls"]:
        cx, cy = px(*c["at"])
        dr.text((cx + 3, cy - 6), f"{c['height']}", fill=(255, 255, 255))
    for z in range(-1400, 1500, 200):
        for x in range(-1800, 1900, 200):
            pass
    img.save(path)


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--check", action="store_true", help="fail when terrain.json is out of date")
    args = p.parse_args()
    design, region, X, Z, H = build()
    out = text(terrain_doc(design, H))
    path = GREYHAVEN / "terrain.json"
    if args.check:
        if not path.exists() or path.read_text() != out:
            print("terrain.json is out of date: run content/greyhaven/authoring/region_design.py")
            return 1
        print("terrain.json is up to date")
        return 0
    path.write_text(out)
    OUT.mkdir(parents=True, exist_ok=True)
    preview(design, region, X, Z, H, OUT / "design.png")
    print(f"wrote {path} ({H.shape[1]} × {H.shape[0]}, {H.min():.0f}..{H.max():.0f} m) and {OUT / 'design.png'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

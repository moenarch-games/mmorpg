"""Blender-only Greyhaven Outpost placement bridge.

Reads live core/scenery context and writes the existing validated layout format.
Blender's meshes are editing guides: gameplay collision and asset definitions
remain owned by mmorpg-core and asset-tooling respectively.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import tempfile

EDITABLE = "Greyhaven Editable"
CONTEXT = "Greyhaven Context"
PREFABS = "Greyhaven Prefabs"
KINDS = {"house", "lamp", "signpost", "flowers"}
TURN = 2.0 * math.pi


def build_document(document: dict, base_houses: list[dict], records: list[dict]) -> tuple[dict, dict[str, str]]:
    """Pure placement serialization; Blender supplies the transform records."""
    if document.get("version") != 1 or document.get("generator") != "greyhaven-15":
        raise ValueError("Unknown Greyhaven layout version or generator")
    base = {item["id"]: item["position"] for item in base_houses}
    used_houses: set[int] = set()
    used_decorations: set[str] = set()
    collision: list[dict] = []
    decoration: list[dict] = []
    assigned: dict[str, str] = {}
    next_house = 60000
    next_decoration = 1

    for record in sorted(records, key=lambda entry: entry["name"]):
        kind = record["kind"]
        if kind not in KINDS:
            raise ValueError(f"Unsupported placement kind: {kind}")
        raw_id = str(record.get("id", ""))
        position = record["position"]
        if not all(type(value) is int for value in position) or len(position) != 2:
            raise ValueError(f"Invalid centimetre position: {record['name']}")
        if kind == "house":
            requested = int(raw_id) if raw_id.isdecimal() else None
            if requested in used_houses or requested not in base and not (
                requested is not None and 60000 <= requested < 65000
            ):
                requested = None
            if requested is None:
                while next_house in used_houses:
                    next_house += 1
                if next_house >= 65000:
                    raise ValueError("No free authored house IDs")
                requested = next_house
                next_house += 1
            used_houses.add(requested)
            assigned[record["name"]] = str(requested)
            if requested not in base or position != base[requested]:
                collision.append({"id": requested, "position": position})
        else:
            # Duplicating a Blender object also duplicates its custom properties.
            # Preserve the first ID in object-name order and assign later copies.
            requested = raw_id
            valid = bool(requested) and len(requested) <= 80 and all(
                char.isascii() and (char.isalnum() or char == "-") for char in requested
            )
            if not valid or requested in used_decorations:
                while f"authored-{next_decoration:04d}" in used_decorations:
                    next_decoration += 1
                requested = f"authored-{next_decoration:04d}"
                next_decoration += 1
            used_decorations.add(requested)
            assigned[record["name"]] = requested
            decoration.append({
                "id": requested,
                "kind": kind,
                "position": position,
                "yaw": record["yaw"] % 65536,
            })

    for house_id in sorted(base):
        if house_id not in used_houses:
            collision.append({"id": house_id, "position": base[house_id], "removed": True})
    return ({
        "version": document["version"],
        "generator": document["generator"],
        "collision": sorted(collision, key=lambda entry: entry["id"]),
        "decoration": sorted(decoration, key=lambda entry: entry["id"]),
    }, assigned)


def blender_position(position: list[int]) -> tuple[float, float, float]:
    # Greyhaven is Y-up and +Z south; Blender is Z-up and +Y north.
    return position[0] / 100, -position[2] / 100, position[1] / 100


def import_scene(args: argparse.Namespace) -> None:
    import bpy

    data = json.loads(Path(args.context).read_text(encoding="utf-8"))
    layout_source = Path(args.layout).read_bytes()
    layout = json.loads(layout_source)

    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for group in list(bpy.data.collections):
        if group.name != bpy.context.scene.collection.name:
            bpy.data.collections.remove(group)

    groups = {}
    for name in (EDITABLE, CONTEXT, PREFABS):
        group = bpy.data.collections.new(name)
        bpy.context.scene.collection.children.link(group)
        groups[name] = group

    materials = {}

    def material(kind: str, color: tuple[float, float, float]):
        if kind not in materials:
            mat = bpy.data.materials.new(f"Greyhaven / {kind}")
            mat.diffuse_color = (*color, 1.0)
            materials[kind] = mat
        return materials[kind]

    colors = {
        "house": (0.66, 0.42, 0.26),
        "lamp": (0.95, 0.73, 0.33),
        "signpost": (0.65, 0.48, 0.30),
        "flowers": (0.72, 0.42, 0.64),
        "tree": (0.22, 0.43, 0.24),
    }

    def box(name: str, kind: str, position, half, group, yaw: int = 0):
        bpy.ops.mesh.primitive_cube_add(size=1)
        obj = bpy.context.object
        obj.name = name
        for coll in tuple(obj.users_collection):
            coll.objects.unlink(obj)
        group.objects.link(obj)
        obj.location = blender_position([position[0], position[1] + half[1], position[2]])
        obj.dimensions = (
            max(0.12, half[0] * 2 / 100),
            max(0.12, half[2] * 2 / 100),
            max(0.12, half[1] * 2 / 100),
        )
        bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
        obj.rotation_euler[2] = yaw / 65536 * TURN
        obj.data.materials.append(material(kind, colors.get(kind, colors["tree"])))
        return obj

    def editable(obj, kind: str, placement_id: str):
        obj["gh_kind"] = kind
        obj["gh_id"] = placement_id
        obj["gh_vertical"] = float(obj.location.z)

    terrain = data["terrain"]
    columns, rows = terrain["columns"], terrain["rows"]
    origin_x, origin_z = terrain["origin"]
    step = terrain["step"]
    vertices = []
    faces = []
    for row in range(rows):
        for col in range(columns):
            i = row * columns + col
            vertices.append(((origin_x + col * step) / 100,
                             -(origin_z + row * step) / 100,
                             terrain["heights"][i] / 100))
            if row and col:
                a = (row - 1) * columns + col - 1
                faces.append((a, a + columns, a + 1))
                faces.append((a + 1, a + columns, a + columns + 1))
    mesh = bpy.data.meshes.new("Greyhaven ground")
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    ground = bpy.data.objects.new("Terrain / visual guide (not editable)", mesh)
    groups[CONTEXT].objects.link(ground)
    ground.data.materials.append(material("ground", (0.35, 0.52, 0.29)))
    ground.hide_select = True

    for prop in data["context"]:
        obj = box(prop["name"], prop["kind"], prop["position"], prop["half"],
                  groups[CONTEXT], prop["yaw"])
        obj.hide_select = True

    for road in data["roads"]:
        curve = bpy.data.curves.new(f"Road / {road['name']}", "CURVE")
        curve.dimensions = "3D"
        curve.bevel_depth = max(0.02, road["half"] / 100)
        curve.resolution_u = 1
        spline = curve.splines.new("POLY")
        spline.points.add(len(road["points"]) - 1)
        for point, (x, z) in zip(spline.points, road["points"]):
            point.co = (x / 100, -z / 100, 0.025, 1)
        obj = bpy.data.objects.new(f"Road / {road['name']}", curve)
        groups[CONTEXT].objects.link(obj)
        obj.hide_select = True

    for house in data["houses"]:
        x, z = house["position"]
        obj = box(f"House / {house['id']}", "house",
                  [x, house["feet"], z], house["half"], groups[EDITABLE])
        editable(obj, "house", str(house["id"]))

    sizes = {
        "lamp": [30, 150, 30], "signpost": [45, 120, 25],
        "flowers": [40, 25, 40], "house": data["base_houses"][0]["half"],
    }
    for item in data["decoration"]:
        kind = item["kind"]
        x, z = item["position"]
        obj = box(f"{kind.title()} / {item['id']}", kind,
                  [x, 0, z], sizes[kind], groups[EDITABLE], item["yaw"])
        editable(obj, kind, item["id"])

    # Blender's ordinary duplicate/move tools are the editor. Prefabs are
    # templates, not a separate custom modeling UI or runtime dependency.
    for index, kind in enumerate(("house", "lamp", "signpost", "flowers")):
        obj = box(f"Prefab / {kind}", kind,
                  [4_200 + index * 400, 0, -1_200], sizes[kind], groups[PREFABS])
        editable(obj, kind, "")

    scene = bpy.context.scene
    scene["gh_source_sha256"] = hashlib.sha256(layout_source).hexdigest()
    scene["gh_base_houses"] = json.dumps(data["base_houses"], sort_keys=True)
    scene["gh_layout_path"] = str(Path(args.layout).resolve())
    bpy.ops.wm.save_as_mainfile(filepath=str(Path(args.blend).resolve()))
    print(f"Greyhaven editable Outpost created: {args.blend}")


def export_scene(args: argparse.Namespace) -> None:
    import bpy

    scene = bpy.context.scene
    layout_path = Path(args.layout)
    source = layout_path.read_bytes()
    digest = hashlib.sha256(source).hexdigest()
    if scene.get("gh_source_sha256") != digest:
        raise ValueError("Saved Greyhaven layout changed since this scene opened; re-import before exporting")
    layout = json.loads(source)
    base_houses = json.loads(scene["gh_base_houses"])
    group = bpy.data.collections.get(EDITABLE)
    if group is None:
        raise ValueError("Missing Greyhaven Editable collection")
    records = []
    objects = {}
    for obj in group.objects:
        if obj.type != "MESH" or not obj.get("gh_kind"):
            continue
        if any(abs(value - 1) > 0.0001 for value in obj.scale):
            raise ValueError(f"{obj.name}: resize is not part of the placement format")
        if abs(obj.location.z - obj.get("gh_vertical", 0.0)) > 0.01:
            raise ValueError(f"{obj.name}: vertical movement is not supported by this format")
        if any(abs(obj.rotation_euler[i]) > 0.0001 for i in (0, 1)):
            raise ValueError(f"{obj.name}: only yaw rotation is supported")
        kind = str(obj["gh_kind"])
        if kind == "house" and abs(math.sin(obj.rotation_euler.z / 2)) > 0.0001:
            raise ValueError(f"{obj.name}: house collision is axis aligned; do not rotate it")
        records.append({
            "name": obj.name,
            "kind": kind,
            "id": str(obj.get("gh_id", "")),
            "position": [round(obj.location.x * 100), round(-obj.location.y * 100)],
            "yaw": round(obj.rotation_euler.z / TURN * 65536) % 65536,
        })
        objects[obj.name] = obj

    updated, ids = build_document(layout, base_houses, records)
    candidate = (json.dumps(updated, indent=2, ensure_ascii=False) + "\n").encode("utf-8")
    fd, staging = tempfile.mkstemp(prefix=".greyhaven-layout-", suffix=".json", dir=layout_path.parent)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(candidate)
        subprocess.run(
            ["cargo", "run", "--quiet", "--locked", "-p", "mmorpg-scenery",
             "--example", "validate-layout", "--", staging],
            cwd=args.root, check=True,
        )
        if hashlib.sha256(layout_path.read_bytes()).hexdigest() != digest:
            raise ValueError("Greyhaven layout changed during validation; nothing was replaced")
        os.replace(staging, layout_path)
        for name, placement_id in ids.items():
            objects[name]["gh_id"] = placement_id
        scene["gh_source_sha256"] = hashlib.sha256(candidate).hexdigest()
        bpy.ops.wm.save_as_mainfile(filepath=bpy.data.filepath)
        print(f"Validated Greyhaven layout saved: {layout_path}")
        print("Rebuild/restart game clients to consume the updated immutable content.")
    finally:
        if os.path.exists(staging):
            os.unlink(staging)


def main() -> None:
    raw = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("import", "export"))
    parser.add_argument("--layout", required=True)
    parser.add_argument("--context")
    parser.add_argument("--blend")
    parser.add_argument("--root")
    args = parser.parse_args(raw)
    if args.mode == "import":
        if not args.context or not args.blend:
            parser.error("import needs --context and --blend")
        import_scene(args)
    else:
        if not args.root:
            parser.error("export needs --root")
        export_scene(args)


if __name__ == "__main__":
    main()

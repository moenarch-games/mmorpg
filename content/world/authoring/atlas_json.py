"""The one layout for content/world/*.json, shared by atlas.py and the Blender adapter so an edit
from either produces a minimal diff. Standard library only: it runs inside Blender too."""

import json
from pathlib import Path


def format_json(data, indent=0):
    """Short scalar lists and flat objects nested two levels deep stay on one line."""
    pad = "  " * indent
    if isinstance(data, dict):
        flat = json.dumps(data, ensure_ascii=False)
        if indent >= 2 and len(flat) <= 160:
            return flat
        items = [f"{pad}  {json.dumps(k)}: {format_json(v, indent + 1)}" for k, v in data.items()]
        return "{\n" + ",\n".join(items) + f"\n{pad}}}"
    if isinstance(data, list):
        flat = json.dumps(data, ensure_ascii=False)
        if len(flat) <= 100 or all(isinstance(x, (int, float)) or is_point(x) for x in data):
            return flat
        items = [f"{pad}  {format_json(v, indent + 1)}" for v in data]
        return "[\n" + ",\n".join(items) + f"\n{pad}]"
    return json.dumps(data, ensure_ascii=False)


def is_point(x):
    return isinstance(x, list) and len(x) == 2 and all(isinstance(v, (int, float)) for v in x)


def write_json(path: Path, data) -> bool:
    """Atomically replace `path` with `data` in the canonical layout; False when nothing changed."""
    text = format_json(data) + "\n"
    if json.loads(text) != data:
        raise ValueError(f"{path}: canonical layout does not round-trip")
    if path.exists() and path.read_text() == text:
        return False
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(text)
    tmp.replace(path)
    return True

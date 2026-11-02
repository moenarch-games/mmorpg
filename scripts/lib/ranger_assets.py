"""Small contract checks for the cached Ranger appearance used by the native client."""

import json
from pathlib import Path


CLASS_NAMES = ("warden", "ranger", "arcanist", "necromancer", "rogue", "cleric", "druid")


def valid_male_appearance(path: Path) -> bool:
    """The native build needs every class outfit in the downloaded male descriptor."""
    try:
        descriptor = json.loads(path.read_text())
    except (OSError, ValueError):
        return False
    if not isinstance(descriptor, dict) or descriptor.get("schema") != "humanoid-appearance-v1":
        return False
    if descriptor.get("character") != "humanoid.ranger.male":
        return False
    presets = descriptor.get("presets")
    if not isinstance(presets, list) or not isinstance(descriptor.get("outfits"), list):
        return False
    ids = {preset["id"] for preset in presets if isinstance(preset, dict)
           and isinstance(preset.get("id"), str)}
    return all(f"class.{name}" in ids for name in CLASS_NAMES)

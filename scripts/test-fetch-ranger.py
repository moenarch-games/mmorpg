#!/usr/bin/env python3
"""Hermetic checks for Ranger download-cache compatibility."""

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


spec = importlib.util.spec_from_file_location("fetch_ranger", Path(__file__).with_name("fetch-ranger.py"))
fetch_ranger = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fetch_ranger)


class RangerCacheTest(unittest.TestCase):
    def test_female_knight_entries_are_selected_only_when_published(self):
        figure = fetch_ranger.Figure("humanoid.ranger.female", fetch_ranger.FEMALE)
        self.assertNotIn(("equipment.cap.ranger", "default"), figure.pairs)
        self.assertNotIn(("equipment.helm.ironcrest", "default"), figure.pairs)
        self.assertNotIn(("armor.knight", "default"), fetch_ranger.selected(figure, {"entries": []}))
        entries = [
            {"key": "humanoid.ranger.female", "variant": "outfit.knight"},
            {"key": "humanoid.ranger.female", "variant": "outfit.knight.skin.light"},
            {"key": "armor.knight", "variant": "default"},
        ]
        selected = fetch_ranger.selected(figure, {"entries": entries})
        for entry in entries:
            self.assertIn((entry["key"], entry["variant"]), selected)

    def test_missing_or_incompatible_class_presets_invalidate_the_cache(self):
        with tempfile.TemporaryDirectory(prefix="ranger-cache-") as temporary:
            directory = Path(temporary)
            figure = fetch_ranger.Figure("humanoid.ranger.male", {"humanoid.ranger.male": {"default"}})
            figure.dest = directory
            appearance = "humanoid.ranger.male.default.humanoid-appearance.json"
            (directory / "RELEASE").write_text(figure.release + "\n")
            (directory / "export.ref.json").write_bytes(b"revision")
            (directory / "humanoid-ranger-male.index.json").write_text(json.dumps({
                "entries": [{"key": "humanoid.ranger.male", "variant": "default", "files": [appearance]}]
            }))
            valid = {
                "schema": "humanoid-appearance-v1", "character": "humanoid.ranger.male", "outfits": [],
                "presets": [{"id": f"class.{name}"} for name in fetch_ranger.CLASS_NAMES],
            }
            (directory / appearance).write_text(json.dumps(valid))
            self.assertTrue(fetch_ranger.complete(figure, b"revision"))
            for descriptor in ({"outfits": []}, {**valid, "presets": []},
                               {**valid, "schema": "unknown"}, {**valid, "presets": [{"id": []}]}):
                (directory / appearance).write_text(json.dumps(descriptor))
                self.assertFalse(fetch_ranger.complete(figure, b"revision"))


if __name__ == "__main__":
    unittest.main(verbosity=2)

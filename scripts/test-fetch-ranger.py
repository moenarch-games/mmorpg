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

    def test_male_hair_and_beards_are_selected_only_while_the_release_offers_them(self):
        figure = fetch_ranger.Figure("humanoid.ranger.male", fetch_ranger.MALE)
        # The hair kit is their source (#677); the release may drop its own (asset-tooling #517).
        self.assertNotIn(("facial-hair.beards", "default"), fetch_ranger.selected(figure, {"entries": []}))
        entries = [{"key": "facial-hair.beards", "variant": "default"}, {"key": "hair.long", "variant": "hair-1"},
                   {"key": "hair.long", "variant": "hair-2"}]
        selected = fetch_ranger.selected(figure, {"entries": entries})
        self.assertIn(("facial-hair.beards", "default"), selected)
        self.assertIn(("hair.long", "hair-1"), selected)
        self.assertNotIn(("hair.long", "hair-2"), selected)

    def test_the_figures_items_come_from_the_equipment_release(self):
        figure = {"schema": "asset-downloads-v1", "release": "humanoid-ranger-male", "entries": [
            {"key": "humanoid.ranger.male", "variant": "default"}, {"key": "equipment.sword.arming", "variant": "default", "old": True}]}
        equipment = {"schema": "asset-downloads-v1", "release": "humanoid-equipment", "entries": [
            {"key": "equipment.sword.arming", "variant": "default"}, {"key": "equipment.venom-flask", "variant": "lod1"}]}
        merged = fetch_ranger.merge_equipment(figure, equipment)
        self.assertEqual(merged["release"], "humanoid-ranger-male")
        self.assertEqual(merged["entries"], [figure["entries"][0], *equipment["entries"]])
        self.assertEqual(fetch_ranger.EQUIPMENT.release, "humanoid-equipment")

    def test_missing_or_incompatible_class_presets_invalidate_the_cache(self):
        with tempfile.TemporaryDirectory(prefix="ranger-cache-") as temporary:
            directory = Path(temporary)
            figure = fetch_ranger.Figure("humanoid.ranger.male", {"humanoid.ranger.male": {"default"}})
            figure.dest = directory
            appearance = "humanoid.ranger.male.default.humanoid-appearance.json"
            (directory / "RELEASE").write_text(figure.release + "\n")
            (directory / "export.ref.json").write_bytes(b"revision")
            (directory / fetch_ranger.EQUIPMENT_REF).write_bytes(b"equipment")
            (directory / "humanoid-ranger-male.index.json").write_text(json.dumps({
                "entries": [{"key": "humanoid.ranger.male", "variant": "default", "files": [appearance]}]
            }))
            valid = {
                "schema": "humanoid-appearance-v1", "character": "humanoid.ranger.male", "outfits": [],
                "presets": [{"id": f"class.{name}"} for name in fetch_ranger.CLASS_NAMES],
            }
            (directory / appearance).write_text(json.dumps(valid))
            self.assertTrue(fetch_ranger.complete(figure, b"revision"))
            # A newer equipment upload (asset-tooling #517) refreshes the copy too.
            self.assertTrue(fetch_ranger.complete(figure, b"revision", b"equipment"))
            self.assertFalse(fetch_ranger.complete(figure, b"revision", b"newer equipment"))
            for descriptor in ({"outfits": []}, {**valid, "presets": []},
                               {**valid, "schema": "unknown"}, {**valid, "presets": [{"id": []}]}):
                (directory / appearance).write_text(json.dumps(descriptor))
                self.assertFalse(fetch_ranger.complete(figure, b"revision"))


if __name__ == "__main__":
    unittest.main(verbosity=2)

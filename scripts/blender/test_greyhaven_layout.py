"""Serializer regression tests; runs with ordinary Python, without Blender."""
import unittest

from greyhaven_layout import build_document


BASE = [
    {"id": 103, "position": [-1200, 800]},
    {"id": 104, "position": [1200, 800]},
]
SOURCE = {
    "version": 1,
    "generator": "greyhaven-15",
    "collision": [],
    "decoration": [
        {"id": "outpost-lamp-1", "kind": "lamp", "position": [-1800, 1000], "yaw": 0}
    ],
}


class LayoutExportTests(unittest.TestCase):
    def test_unchanged_base_and_saved_lamp(self):
        records = [
            {"name": "House / 103", "kind": "house", "id": "103", "position": [-1200, 800], "yaw": 0},
            {"name": "House / 104", "kind": "house", "id": "104", "position": [1200, 800], "yaw": 0},
            {"name": "Lamp / 1", "kind": "lamp", "id": "outpost-lamp-1", "position": [-1800, 1000], "yaw": 0},
        ]
        output, ids = build_document(SOURCE, BASE, records)
        self.assertEqual(output["collision"], [])
        self.assertEqual(output["decoration"], SOURCE["decoration"])
        self.assertEqual(ids["House / 103"], "103")

    def test_move_delete_duplicate_and_persistent_ids(self):
        records = [
            {"name": "House / 103", "kind": "house", "id": "103", "position": [-1300, 800], "yaw": 0},
            {"name": "House / 103.001", "kind": "house", "id": "103", "position": [-2200, 800], "yaw": 0},
            {"name": "Lamp / 1", "kind": "lamp", "id": "outpost-lamp-1", "position": [-1800, 1000], "yaw": 3000},
            {"name": "Lamp / 1.001", "kind": "lamp", "id": "outpost-lamp-1", "position": [1800, 1000], "yaw": 0},
        ]
        output, ids = build_document(SOURCE, BASE, records)
        self.assertEqual(ids["House / 103.001"], "60000")
        self.assertEqual(ids["Lamp / 1.001"], "authored-0001")
        self.assertEqual(output["collision"], [
            {"id": 103, "position": [-1300, 800]},
            {"id": 104, "position": [1200, 800], "removed": True},
            {"id": 60000, "position": [-2200, 800]},
        ])
        self.assertEqual(len(output["decoration"]), 2)

    def test_unknown_prefab_fails(self):
        with self.assertRaisesRegex(ValueError, "Unsupported"):
            build_document(SOURCE, BASE, [
                {"name": "tree", "kind": "tree", "position": [0, 0], "yaw": 0}
            ])


if __name__ == "__main__":
    unittest.main()

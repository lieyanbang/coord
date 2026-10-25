from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from aggregate_results import build_indexes
from prepare_initial_tasks import InputError, contained_file, validate_inventory
from validate_result import validate_result


class PhaseGToolsTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="coord-phase-g-")
        self.root = Path(self.temp.name)
        self.refs = self.root / "refs"
        (self.refs / "arknights" / "icons").mkdir(parents=True)
        (self.refs / "operators.csv").write_text("game,name,path\n", encoding="utf-8")
        (self.refs / "arknights" / "icons" / "Test.png").write_bytes(b"fixture")
        self.results = self.root / "results"
        self.results.mkdir()

    def tearDown(self) -> None:
        self.temp.cleanup()

    def result(self, group: str, image_id: str) -> dict:
        return {
            "schema_version": 1,
            "group_id": group,
            "agent_id": "A0",
            "attempt_no": 1,
            "source_image_ids": [image_id],
            "characters": [
                {
                    "local_character_id": "local-one",
                    "image_ids": [image_id],
                    "representative_image_id": image_id,
                    "visual_description": "Short silver hair, amber eyes, dark coat and a visible animal ear.",
                    "game": "arknights",
                    "reference_match": {
                        "match_status": "candidate",
                        "candidates": [
                            {
                                "game": "arknights",
                                "operator_name": "Test Operator",
                                "icon_path": "refs/arknights/icons/Test.png",
                            }
                        ],
                        "checked_paths": [
                            "refs/operators.csv",
                            "refs/arknights/icons/Test.png",
                        ],
                        "conclusion": "The checked icon is similar, but the visible clothing remains inconclusive.",
                    },
                    "confidence": 0.72,
                    "notes": "Compare the ear shape and collar before treating this as a match.",
                }
            ],
            "uncertain": [],
            "general_notes": "First-pass visual grouping only.",
        }

    def test_result_requires_exact_coverage_and_existing_refs_paths(self) -> None:
        result = self.result("00", "7")
        expected = {"group_id": "00", "agent_id": "A0", "attempt_no": 1, "source_image_ids": ["7"]}
        self.assertEqual(validate_result(result, expected, self.refs), [])

        result["characters"][0]["reference_match"]["candidates"][0]["icon_path"] = "refs/../secret.txt"
        errors = validate_result(result, expected, self.refs)
        self.assertTrue(any("unsafe reference path" in error for error in errors))

    def test_logical_worker_result_requires_canonical_and_matching_owner(self) -> None:
        result = self.result("00", "7")
        result["agent_id"] = "emote-ABCD"
        expected = {"group_id": "00", "agent_id": "emote-ABCD", "attempt_no": 1, "source_image_ids": ["7"]}
        self.assertEqual(validate_result(result, expected, self.refs), [])
        for owner in ("emote-abcd", "emote-ABC", "emote-0000", "emote-GGGG"):
            result["agent_id"] = owner
            self.assertTrue(any("agent_id" in error for error in validate_result(result, expected, self.refs)))

    def test_result_rejects_missing_image_coverage_and_stale_attempt(self) -> None:
        result = self.result("00", "7")
        expected = {"group_id": "00", "agent_id": "A1", "attempt_no": 2, "source_image_ids": ["7", "8"]}
        errors = validate_result(result, expected, self.refs)
        self.assertTrue(any("agent_id" in error for error in errors))
        self.assertTrue(any("attempt_no" in error for error in errors))
        self.assertTrue(any("coverage mismatch" in error for error in errors))

    def test_malformed_untrusted_json_returns_errors_instead_of_raising(self) -> None:
        result = self.result("00", "7")
        result["agent_id"] = {"unexpected": "object"}
        result["source_image_ids"] = [{}]
        result["characters"][0]["image_ids"] = [{}]
        result["characters"][0]["game"] = []
        result["characters"][0]["reference_match"]["match_status"] = []
        result["characters"][0]["reference_match"]["checked_paths"] = [{}]
        expected = {"group_id": "00", "source_image_ids": ["7"]}
        errors = validate_result(result, expected, self.refs)
        self.assertTrue(errors)

    def test_source_path_rejects_traversal(self) -> None:
        with self.assertRaises(InputError):
            contained_file(self.root, "../outside.png")

    def test_initializer_rejects_incomplete_manifest_before_preparing_tasks(self) -> None:
        source = self.root / "incomplete-source"
        source.mkdir()
        (source / "image_manifest.csv").write_text(
            "id,source_name,sha256,status,duplicate_of,task\n"
            "1,1.png,00,unique,,00\n",
            encoding="utf-8",
        )
        (source / "task_inventory.csv").write_text("task,count\n00,10\n", encoding="utf-8")

        with self.assertRaisesRegex(InputError, "expected 1218 manifest rows"):
            validate_inventory(source)

    def test_aggregator_emits_review_candidates_without_global_merge(self) -> None:
        request = {
            "tasks": [
                {"payload": {"group_id": "00", "source_image_ids": ["7"]}},
                {"payload": {"group_id": "01", "source_image_ids": ["8"]}},
            ]
        }
        request_path = self.root / "request.json"
        request_path.write_text(json.dumps(request), encoding="utf-8")
        for group, image_id in (("00", "7"), ("01", "8")):
            (self.results / f"{group}.json").write_text(
                json.dumps(self.result(group, image_id)), encoding="utf-8"
            )

        index, review = build_indexes(request_path, self.results, self.refs)
        self.assertEqual(index["state"], "STRUCTURALLY_COMPLETE")
        self.assertEqual(review["candidate_count"], 1)
        self.assertNotIn("global_character_id", review["candidates"][0])

        (self.results / "01.json").unlink()
        index, review = build_indexes(request_path, self.results, self.refs)
        self.assertEqual(index["state"], "PARTIAL")
        self.assertEqual(index["missing_groups"], ["01"])
        self.assertEqual(review["missing_groups"], ["01"])

    def test_aggregator_includes_three_digit_groups(self) -> None:
        request = {"tasks": [{"payload": {"group_id": group, "source_image_ids": [image_id]}}
                             for group, image_id in (("99", "99"), ("100", "100"), ("120", "120"))]}
        request_path = self.root / "request.json"
        request_path.write_text(json.dumps(request), encoding="utf-8")
        for group, image_id in (("99", "99"), ("100", "100"), ("120", "120")):
            (self.results / f"{group}.json").write_text(
                json.dumps(self.result(group, image_id)), encoding="utf-8"
            )
        index, _ = build_indexes(request_path, self.results, self.refs)
        self.assertEqual(index["state"], "STRUCTURALLY_COMPLETE")
        self.assertEqual(index["available_results"], 3)
        self.assertEqual(index["missing_groups"], [])


if __name__ == "__main__":
    unittest.main()

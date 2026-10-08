"""End-to-end Python records with the fixed Rust offline scenario as control."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import ionoray_geospace as gs
from examples import driver_scenarios as analysis


ROOT = Path(__file__).resolve().parents[2]


class AnalysisTests(unittest.TestCase):
    def test_editable_direct_cases_continue_and_keep_complete_results(self):
        records = list(analysis.direct_records())
        self.assertEqual(len(records), 12)
        self.assertEqual(len({row["id"] for row in records}), 12)
        for model in ("igrf", "iri", "hwm", "msis"):
            first, invalid, following = (row for row in records if row["model"] == model)
            self.assertIn("result", first)
            self.assertEqual(first["result"], getattr(gs, model)(**first["request"]))
            self.assertEqual(invalid["error"]["type"], "ValueError")
            self.assertNotIn("result", invalid)
            self.assertIn("result", following)
            self.assertEqual(following["result"], getattr(gs, model)(**following["request"]))
            self.assertEqual(first["analysis"]["model_version"],
                             first["result"]["provenance"]["version"])
        iri = records[3]
        self.assertIsNone(iri["result"]["point"]["ions"]["cluster_m3"])
        self.assertIn("electron_density_m3", iri["analysis"])
        self.assertIn("magnetic_magnitude_T", records[0]["analysis"])
        self.assertIn("northward_wind_m_s", records[6]["analysis"])
        self.assertIn("mass_density_kg_m3", records[9]["analysis"])

    def test_automatic_records_match_rust_and_survive_closed_session(self):
        binary = ROOT / "target/debug/examples/driver_scenarios"
        self.assertTrue(binary.is_file(), "Build Rust driver_scenarios with Nix first")
        run = subprocess.run([str(binary)], capture_output=True, text=True,
                             timeout=180, check=True)
        rust = {(row["model"], row["scenario_id"]): row
                for row in map(json.loads, run.stdout.splitlines())}
        with tempfile.TemporaryDirectory(prefix="geospace-analysis-test-") as home:
            records = list(analysis.automatic_records(home))
        self.assertEqual(len(records), 12)
        self.assertEqual(len({row["id"] for row in records}), 12)
        scenario_names = {
            "low": "low", "bad": "invalid", "high": "high",
            "quiet_a": "quiet_a", "disturbed_b": "disturbed_b",
            "daily_a": "daily_a", "storm_b": "storm_b",
        }
        for row in records:
            model = row["model"]
            if row["mode"] == "automatic":
                self.assertEqual(row["evaluation"], rust[model, "baseline"]["evaluation"])
            else:
                name = row["id"].split(".")[-1]
                reference = rust[model, scenario_names[name]]
                self.assertEqual(row["input"], reference["input"])
                evidence = "ap_index" if model == "hwm" else "indices"
                self.assertEqual(row[evidence], reference[evidence])
                if name == "bad":
                    self.assertEqual(row["error"]["type"], "ValueError")
                    continue
                self.assertEqual(row["evaluation"], reference["outcome"]["Ok"])
            self.assertEqual(row["analysis"]["model_version"],
                             row["evaluation"]["result"]["provenance"]["version"])
        self.assertIsNone(next(row for row in records if row["id"] == "auto.hwm.quiet_a")["ap_index"])
        self.assertIsNone(next(row for row in records if row["id"] == "auto.iri.high")["indices"]["f107_daily"])
        self.assertEqual(next(row for row in records if row["id"] == "auto.msis.storm_b")
                         ["indices"]["ap_three_hourly"], [])

    def test_cli_example_emits_stable_json_lines(self):
        run = subprocess.run([sys.executable, str(ROOT / "examples/driver_scenarios.py")],
                             cwd=ROOT, capture_output=True, text=True, timeout=180, check=True)
        rows = [json.loads(line) for line in run.stdout.splitlines()]
        self.assertEqual(len(rows), 24)
        self.assertEqual(len({row["id"] for row in rows}), 24)
        self.assertEqual([row["id"] for row in rows[:3]],
                         ["direct.igrf.01", "direct.igrf.bad", "direct.igrf.02"])
        self.assertIn("error", rows[1])
        self.assertIn("result", rows[2])
        self.assertIn("evaluation", rows[-1])


if __name__ == "__main__":
    unittest.main()

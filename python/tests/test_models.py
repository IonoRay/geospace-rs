"""Real extension tests, including comparison with the S3 Rust main."""
import json
import math
from pathlib import Path
import subprocess
import unittest
import ionoray_geospace as gs
from .common import P, IRI, MSIS, baselines


class ModelsTests(unittest.TestCase):
    def test_direct_matches_prepared_full_result(self):
        cases = {"iri": IRI, "hwm": dict(activity="disturbed", current_ap=2.), "msis": MSIS}
        for model, drivers in cases.items():
            with self.subTest(model=model):
                result = getattr(gs, model)(**P, **drivers)
                self.assertEqual(result, baselines()[model].evaluate()["result"])
        self.assertFalse(hasattr(gs, "point"))
        self.assertFalse(hasattr(gs.Session, "evaluate_igrf"))

    def test_rust_python_same_input_units_none_and_provenance(self):
        root = Path(__file__).resolve().parents[2]
        binary = root / "target/debug/examples/driver_scenarios"
        self.assertTrue(binary.is_file(), "Build Rust driver_scenarios with Nix first (see docs/python-models.md)")
        run = subprocess.run([str(binary)], capture_output=True, text=True, timeout=180, check=True)
        rows = [json.loads(line) for line in run.stdout.splitlines()]
        self.assertEqual(len(rows), 15)
        for row in rows[:3]:
            self.assertEqual(row["evaluation"], baselines()[row["model"]].evaluate())
        self.assertEqual(rows[-1]["result"], gs.igrf(**P))
        position = baselines()["iri"].input["query"]["position"]
        self.assertAlmostEqual(position["latitude"], math.pi/6)
        self.assertAlmostEqual(position["longitude"], 2*math.pi/3)
        self.assertEqual(position["altitude"], 300000.)
        self.assertAlmostEqual(gs.igrf(**P)["field"]["magnitude"], 4.1557250565660555e-5)
        # Complete recursive equality above includes all nulls and provenance fields.
        self.assertIsNone(gs.iri(**P, **IRI)["point"]["ions"]["cluster_m3"])

    def test_strict_types_utc_and_driver_shapes(self):
        cases = [
            (TypeError, lambda: gs.igrf(**(P | {"latitude_deg": True}))),
            (TypeError, lambda: gs.iri(**P, **(IRI | {"ig12": True}))),
            (TypeError, lambda: gs.igrf(**P, unknown=1)),
            (TypeError, lambda: gs.hwm(**P, activity="quiet", f107_daily=100)),
            (TypeError, lambda: gs.iri(**P)),
            (TypeError, lambda: gs.hwm(**P)),
            (TypeError, lambda: gs.igrf(P)),
            (TypeError, lambda: gs.msis(**P, **(MSIS | {"ap_history": {"bad": 1}}))),
            (TypeError, lambda: gs.msis(**P, **(MSIS | {"ap_history": {"daily": 4}}))),
            (ValueError, lambda: gs.msis(**P, **MSIS, ap_daily=4)),
            (ValueError, lambda: gs.hwm(**P, activity="quiet", current_ap=2)),
            (ValueError, lambda: gs.hwm(**P, activity="disturbed")),
            (ValueError, lambda: gs.hwm(**P, activity="disturbed", current_ap=-1)),
            (ValueError, lambda: gs.iri(**P, **(IRI | {"f107_daily": -1}))),
            (ValueError, lambda: gs.igrf(**(P | {"latitude_deg": 91}))),
            (ValueError, lambda: gs.igrf(**(P | {"altitude_km": float("nan")}))),
            (ValueError, lambda: gs.igrf(**(P | {"longitude_deg": float("inf")}))),
        ]
        for error, call in cases:
            with self.subTest(call=call), self.assertRaises(error): call()
        for at in ["2020-07-01T12:00:00", "2020-07-01T12:00:00+01:00", "2020-07-01T12:00:00 TAI"]:
            with self.subTest(at=at), self.assertRaises(ValueError): gs.igrf(**(P | {"at": at}))
        for suffix in ["Z", "+00:00", " UTC"]:
            self.assertEqual(gs.igrf(**(P | {"at": "2020-07-01T12:00:00"+suffix})), gs.igrf(**P))

    def test_public_result_types_match_runtime_shapes(self):
        from ionoray_geospace import types
        pairs = [(gs.igrf(**P), types.IgrfResult), (gs.iri(**P, **IRI), types.IriResult),
                 (gs.hwm(**P, activity="quiet"), types.HwmResult), (gs.msis(**P, **MSIS), types.MsisResult)]
        for result, typed in pairs:
            self.assertEqual(set(result), set(typed.__annotations__))


if __name__ == "__main__": unittest.main()

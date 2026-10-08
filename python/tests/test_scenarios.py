"""Prepared copies, evidence, lifecycle and error continuation."""
import unittest
from .common import P, IRI, MSIS, HISTORY, baselines
import ionoray_geospace as gs


class ScenariosTests(unittest.TestCase):
    def test_frozen_properties_and_independent_lifetime(self):
        for name, baseline in baselines().items():
            with self.subTest(model=name):
                original = baseline.evaluate()
                snapshot = baseline.input
                snapshot["query"]["position"]["altitude"] = 0
                evidence = baseline.ap_index if name == "hwm" else baseline.indices
                evidence.clear()
                self.assertEqual(baseline.evaluate(), original)
                with self.assertRaises(AttributeError): baseline.input = {}
                self.assertEqual(baseline.evaluate(), original)

    def test_iri_none_same_value_and_each_field_evidence(self):
        baseline = baselines()["iri"]
        self.assertEqual(baseline.with_overrides(ig12=None).evaluate(), baseline.evaluate())
        for field, value in IRI.items():
            scenario = baseline.with_overrides(**{field: value})
            self.assertIsNone(scenario.indices[field])
            self.assertIsNotNone(baseline.indices[field])
            self.assertEqual(scenario.input, baseline.input)
            self.assertEqual(scenario.evaluate()["result"], baseline.evaluate()["result"])
            for other in IRI:
                if other != field: self.assertEqual(scenario.indices[other], baseline.indices[other])
        self.assertEqual(baseline.with_overrides(rz12=0, ig12=-5).input["drivers"]["ionospheric_index_12_month"], -5)

    def test_activity_switching_and_msis_evidence(self):
        hwm = baselines()["hwm"]
        quiet = hwm.with_activity(activity="quiet")
        self.assertIsNone(quiet.ap_index)
        self.assertEqual(quiet.evaluate(), hwm.with_activity(activity="disturbed", current_ap=40).with_activity(activity="quiet").evaluate())
        msis = baselines()["msis"]
        storm = msis.with_overrides(ap_history=HISTORY)
        self.assertEqual(storm.evaluate()["result"], msis.evaluate()["result"])
        self.assertIsNone(storm.indices["ap_daily"])
        self.assertEqual(storm.indices["ap_three_hourly"], [])
        self.assertEqual(len(msis.indices["ap_three_hourly"]), 20)
        daily = msis.with_overrides(ap_daily=4)
        self.assertEqual(daily.evaluate(), storm.with_overrides(ap_daily=4).evaluate())
        for field in ["f107a", "f107_previous_day"]:
            scenario = msis.with_overrides(**{field: MSIS[field]})
            self.assertIsNone(scenario.indices[field])
            self.assertEqual(scenario.indices["ap_three_hourly"], msis.indices["ap_three_hourly"])
        self.assertEqual(msis.with_overrides(ap_daily=0).input["drivers"]["geomagnetic_activity"], {"Daily": 0.})

    def test_invalid_scenario_then_success(self):
        for name, baseline in baselines().items():
            values = []
            for value in [0., -1., 40.]:
                scenario = (baseline.with_activity(activity="disturbed", current_ap=value) if name == "hwm"
                            else baseline.with_overrides(**{("f107_daily" if name == "iri" else "ap_daily"): value}))
                try: values.append(scenario.evaluate())
                except ValueError: values.append(None)
            self.assertIsNotNone(values[0]); self.assertIsNone(values[1]); self.assertIsNotNone(values[2])


if __name__ == "__main__": unittest.main()

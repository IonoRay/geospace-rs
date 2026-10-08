"""Four-model Python analysis with editable requests and complete JSON records.

Run from the repository root after scripts/prepare_python_debug.py. Input
coordinates are deg/km; serialized inputs/results use SI and flux uses sfu.
"""

import json
import sys
import tempfile

import ionoray_geospace as gs


POINT = dict(at="2020-07-01T12:00:00Z", latitude_deg=30.0,
             longitude_deg=120.0, altitude_km=300.0)
IRI = dict(rz12=5.940666666666667, ig12=-5.526666666666667,
           f107_daily=71.2, f107_81_day=72.1)
AP_HISTORY = dict(daily=4.0, current=2.0, three_hours_ago=4.0,
                  six_hours_ago=3.0, nine_hours_ago=4.0,
                  average_12_to_33_hours=3.375, average_36_to_57_hours=2.5)
MSIS = dict(f107a=69.9543209876543, f107_previous_day=68.1,
            ap_history=AP_HISTORY)

# Edit these requests for direct analysis. IDs remain stable across runs.
# Each model has a valid request, invalid request, then another valid request.
DIRECT_CASES = [
    ("igrf.01", "igrf", POINT),
    ("igrf.bad", "igrf", POINT | {"latitude_deg": 91.0}),
    ("igrf.02", "igrf", POINT | {"altitude_km": 310.0}),
    ("iri.01", "iri", POINT | IRI),
    ("iri.bad", "iri", POINT | IRI | {"f107_daily": -1.0}),
    ("iri.02", "iri", POINT | IRI | {"f107_daily": 70.0}),
    ("hwm.01", "hwm", POINT | {"activity": "disturbed", "current_ap": 2.0}),
    ("hwm.bad", "hwm", POINT | {"activity": "disturbed", "current_ap": -1.0}),
    ("hwm.02", "hwm", POINT | {"activity": "quiet"}),
    ("msis.01", "msis", POINT | MSIS),
    ("msis.bad", "msis", POINT | {"f107a": MSIS["f107a"],
                                    "f107_previous_day": MSIS["f107_previous_day"],
                                    "ap_daily": -1.0}),
    ("msis.02", "msis", POINT | {"f107a": MSIS["f107a"],
                                   "f107_previous_day": MSIS["f107_previous_day"],
                                   "ap_daily": 4.0}),
]


def analysis_columns(model, result):
    """Small unit-labelled view; the full result stays in the same record."""
    if model == "igrf":
        key, value = "magnetic_magnitude_T", result["field"]["magnitude"]
    elif model == "iri":
        key, value = "electron_density_m3", result["point"]["electron_density_m3"]
    elif model == "hwm":
        key, value = "northward_wind_m_s", result["wind"]["northward"]
    else:
        key, value = "mass_density_kg_m3", result["atmosphere"]["mass_density_kg_m3"]
    return {"model_version": result["provenance"]["version"], key: value}


def failure(error):
    detail = {"type": type(error).__name__, "message": str(error)}
    if isinstance(error, gs.GeospaceError):
        detail["code"] = error.code
    return detail


def direct_records(cases=DIRECT_CASES):
    for case_id, model, request in cases:
        record = {"id": f"direct.{case_id}", "model": model,
                  "mode": "direct", "request": request}
        try:
            result = getattr(gs, model)(**request)
            record["result"] = result
            record["analysis"] = analysis_columns(model, result)
        except (TypeError, ValueError, gs.GeospaceError) as error:
            record["error"] = failure(error)
        yield record


def scenario_cases(prepared):
    # Final StormTime case reuses the baseline's seven resolved Ap values.
    history = prepared["msis"].input["drivers"]["geomagnetic_activity"]["StormTime"]
    return [
        ("iri.low", "iri", {"f107_daily": 70.0}),
        ("iri.bad", "iri", {"f107_daily": -1.0}),
        ("iri.high", "iri", {"f107_daily": 150.0}),
        ("hwm.quiet_a", "hwm", {"activity": "quiet"}),
        ("hwm.bad", "hwm", {"activity": "disturbed", "current_ap": -1.0}),
        ("hwm.disturbed_b", "hwm", {"activity": "disturbed", "current_ap": 40.0}),
        ("msis.daily_a", "msis", {"ap_daily": 4.0}),
        ("msis.bad", "msis", {"ap_daily": -1.0}),
        ("msis.storm_b", "msis", {"ap_history": history}),
    ]


def automatic_records(home):
    # One Session resolves all three models; Prepared survives its close.
    with gs.Session(home=home, data_policy="offline") as session:
        prepared = {model: getattr(session, f"prepare_{model}")(**POINT)
                    for model in ("iri", "hwm", "msis")}
        automatic = {model: getattr(session, f"evaluate_{model}")(**POINT)
                     for model in prepared}
    # Breakpoint 1: Session is closed; inspect prepared["iri"].input/indices.
    for model, evaluation in automatic.items():
        yield {"id": f"auto.{model}.baseline", "model": model, "mode": "automatic",
               "request": POINT, "evaluation": evaluation,
               "analysis": analysis_columns(model, evaluation["result"])}
    for case_id, model, overrides in scenario_cases(prepared):
        baseline = prepared[model]
        record = {"id": f"auto.{case_id}", "model": model, "mode": "scenario",
                  "overrides": overrides}
        try:
            scenario = (baseline.with_activity(**overrides) if model == "hwm"
                        else baseline.with_overrides(**overrides))
            record["input"] = scenario.input
            record["ap_index" if model == "hwm" else "indices"] = (
                scenario.ap_index if model == "hwm" else scenario.indices)
            # Breakpoint 2: inspect input/evidence after the override.
            evaluation = scenario.evaluate()
            record["evaluation"] = evaluation
            record["analysis"] = analysis_columns(model, evaluation["result"])
        except (TypeError, ValueError, gs.GeospaceError) as error:
            record["error"] = failure(error)
        # Breakpoint 3: the next item runs even if this one failed.
        yield record


def main():
    for record in direct_records():
        print(json.dumps(record, allow_nan=False))
    with tempfile.TemporaryDirectory(prefix="geospace-python-") as home:
        print(f"isolated offline home: {home}", file=sys.stderr)
        for record in automatic_records(home):
            print(json.dumps(record, allow_nan=False))


if __name__ == "__main__":
    main()

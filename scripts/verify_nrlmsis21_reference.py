"""Replay Daily and StormTime NRLMSIS 2.1 cases without the Rust adapter.

The pinned, unmodified upstream Fortran is compiled in double precision in a
temporary directory.  The Daily case is also checked against the official
rounded ``msis2.1_test_ref_dp.txt`` row.  The StormTime case uses the upstream
``switch_legacy(9) = -1`` interface and a complete seven-element Ap history.
"""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile


ROOT = Path(__file__).resolve().parents[1]
MODEL = ROOT / "crates/models/msis"
ARCHIVE_SHA256 = "41e47b29f795d36a5cc252b2858aa2a384c4a7323ace3d48d3ea2f2b37a1a6a8"
OFFICIAL_REFERENCE_SHA256 = (
    "59210a442f175b6b3f9e15034856989eea46d54ddb4fe3057a289a77512b7ce3"
)
SOURCES = [
    "msis_constants.F90",
    "msis_utils.F90",
    "msis_init.F90",
    "msis_gfn.F90",
    "msis_tfn.F90",
    "msis_dfn.F90",
    "msis_calc.F90",
    "msis_gtd8d.F90",
]
DRIVER = r"""
program reference
  use msis_init, only: msisinit
  use msis_calc, only: msiscalc
  implicit none
  real(4) :: switches(25)
  real(8) :: ap(7), tn, dn(10), tex

  switches = 1.0
  ap = 6.0d0
  call msisinit(parmpath='', parmfile='msis21.parm', switch_legacy=switches)
  call msiscalc(213.0d0,39510.0d0,399.1d0,-24.4d0,119.1d0, &
      86.5d0,84.8d0,ap,tn,dn,tex)
  write(*,'(A,12ES26.17)') 'DAILY ',tn,tex,dn

  switches = 1.0
  switches(9) = -1.0
  ap = (/4.0d0,2.0d0,4.0d0,3.0d0,4.0d0,3.375d0,2.5d0/)
  call msisinit(parmpath='', parmfile='msis21.parm', switch_legacy=switches)
  call msiscalc(183.0d0,43200.0d0,300.0d0,30.0d0,120.0d0, &
      69.9543209876543d0,68.1d0,ap,tn,dn,tex)
  write(*,'(A,12ES26.17)') 'STORM ',tn,tex,dn
end program
"""


def parse_output(text):
    result = {}
    for line in text.splitlines():
        fields = line.split()
        if fields and fields[0] in {"DAILY", "STORM"}:
            values = [float(value) for value in fields[1:]]
            if len(values) != 12:
                raise ValueError(f"expected 12 outputs for {fields[0]}")
            result[fields[0].lower()] = {
                "temperature_k": values[0],
                "exospheric_temperature_k": values[1],
                "densities_si": values[2:],
            }
    if set(result) != {"daily", "storm"}:
        raise ValueError("missing Daily or StormTime reference output")
    return result


def verify_official_daily_row(reference, daily):
    if hashlib.sha256(reference).hexdigest() != OFFICIAL_REFERENCE_SHA256:
        raise SystemExit("official output SHA-256 mismatch")
    lines = [line for line in reference.decode().splitlines() if line.startswith("  74213")]
    if len(lines) != 1:
        raise SystemExit("official Daily reference row is missing or ambiguous")
    fields = [float(value) for value in lines[0].split()]
    # Official legacy order and units: He, O, N2, O2, Ar, rho in g/cm3,
    # H, N, anomalous O, NO in cm-3, then temperature in K.
    legacy = fields[9:20]
    converted = [
        legacy[5] * 1.0e3,
        legacy[2] * 1.0e6,
        legacy[3] * 1.0e6,
        legacy[1] * 1.0e6,
        legacy[0] * 1.0e6,
        legacy[6] * 1.0e6,
        legacy[4] * 1.0e6,
        legacy[7] * 1.0e6,
        legacy[8] * 1.0e6,
        legacy[9] * 1.0e6,
    ]
    for actual, rounded in zip(daily["densities_si"], converted, strict=True):
        if abs(actual - rounded) > abs(rounded) * 5.0e-4:
            raise SystemExit(f"Daily density differs from official row: {actual} != {rounded}")
    if abs(daily["temperature_k"] - legacy[10]) > 0.005:
        raise SystemExit("Daily temperature differs from official row")


def reference_document(outputs):
    return {
        "release_sha256": ARCHIVE_SHA256,
        "official_output_sha256": OFFICIAL_REFERENCE_SHA256,
        "cases": [
            {
                "id": "daily_official_row_74213",
                "mode": "daily",
                "utc": [1974, 8, 1, 10, 58, 30],
                "position_deg_km": [-24.4, 119.1, 399.1],
                "f107a": 86.5,
                "f107_previous_day": 84.8,
                "ap": [6.0] * 7,
                "output": outputs["daily"],
            },
            {
                "id": "storm_time_offline_snapshot",
                "mode": "storm_time",
                "utc": [2020, 7, 1, 12, 0, 0],
                "position_deg_km": [30.0, 120.0, 300.0],
                "f107a": 69.9543209876543,
                "f107_previous_day": 68.1,
                "ap": [4.0, 2.0, 4.0, 3.0, 4.0, 3.375, 2.5],
                "output": outputs["storm"],
            },
        ],
    }


def compare(actual, expected):
    for actual_case, expected_case in zip(actual["cases"], expected["cases"], strict=True):
        if actual_case.keys() != expected_case.keys():
            raise SystemExit("reference schema mismatch")
        for key in actual_case.keys() - {"output"}:
            if actual_case[key] != expected_case[key]:
                raise SystemExit(f"reference input mismatch for {actual_case['id']}: {key}")
        actual_output = actual_case["output"]
        expected_output = expected_case["output"]
        for key in ["temperature_k", "exospheric_temperature_k"]:
            target = expected_output[key]
            if abs(actual_output[key] - target) > max(abs(target), 1.0) * 5.0e-12:
                raise SystemExit(f"{actual_case['id']} {key} mismatch")
        for index, (value, target) in enumerate(
            zip(actual_output["densities_si"], expected_output["densities_si"], strict=True)
        ):
            if abs(value - target) > max(abs(target) * 5.0e-12, 1.0e-30):
                raise SystemExit(f"{actual_case['id']} density {index} mismatch")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--archive", type=Path, help="local pinned official archive")
    parser.add_argument("--show", action="store_true", help="print the independently computed JSON")
    args = parser.parse_args()
    archive = args.archive or MODEL / "cache/nrlmsis2.1.tar.gz"
    if not archive.is_file():
        raise SystemExit(f"Missing {archive}; provide --archive with the fixed official release (docs/releases.md)")
    if hashlib.sha256(archive.read_bytes()).hexdigest() != ARCHIVE_SHA256:
        raise SystemExit("official archive SHA-256 mismatch")

    with tempfile.TemporaryDirectory(prefix="nrlmsis21-reference-") as directory:
        work = Path(directory)
        with tarfile.open(archive) as source:
            for name in [*SOURCES, "msis21.parm"]:
                (work / name).write_bytes(source.extractfile(name).read())
            official_reference = source.extractfile("msis2.1_test_ref_dp.txt").read()
        (work / "reference.f90").write_text(DRIVER)
        command = ["gfortran", "-O2", "-cpp", "-DDBLE", *SOURCES, "reference.f90", "-o", "reference"]
        if sys.platform == "darwin":
            command.append("-fno-omit-frame-pointer")
        subprocess.run(command, cwd=work, check=True, capture_output=True)
        output = subprocess.run(
            [str(work / "reference")], cwd=work, check=True, capture_output=True, text=True, timeout=30
        ).stdout
    outputs = parse_output(output)
    verify_official_daily_row(official_reference, outputs["daily"])
    actual = reference_document(outputs)
    if args.show:
        print(json.dumps(actual, indent=2))
        return
    expected = json.loads((MODEL / "data/reference.json").read_text())
    compare(actual, expected)
    print("verified NRLMSIS 2.1 Daily and independent StormTime references (12 outputs each)")


if __name__ == "__main__":
    main()

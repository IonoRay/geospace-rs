"""Replay the pinned official interactive program; no network or Rust calls.

Usage: python3 scripts/verify_igrf14_reference.py /path/to/igrf14.f
Prints the reference JSON to stdout, and checks it against the committed fixture.
"""

import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile

SOURCE_SHA256 = "0c4fe0664d837b86c1d1fb4d2a354d6a14a29bbf028b550ab1342e1b828dd2c0"
FIXTURE = Path(__file__).resolve().parents[1] / "crates/models/igrf/data/reference.json"
# year, month, day, decimal year, geodetic latitude/longitude (deg), height (km)
CASES = [
    (2025, 1, 1, 2025.0, 30.0, 120.0, 300.0),
    (1900, 1, 1, 1900.0, 0.0, 0.0, 0.0),
    (1990, 1, 1, 1990.0, -45.0, -75.0, 100.0),
    (2000, 1, 1, 2000.0, 90.0, 0.0, 0.0),
    (2030, 1, 1, 2030.0, -90.0, 179.0, 500.0),
    (1994, 1, 1, 1994.0, 40.0, -75.0, 0.0),
    (1995, 1, 1, 1995.0, 40.0, -75.0, 0.0),
    (2020, 7, 2, 2020.5, 30.0, 120.0, 300.0),
    (2027, 1, 1, 2027.0, 30.0, 120.0, 300.0),
]


def main():
    source = Path(sys.argv[1]).resolve()
    if hashlib.sha256(source.read_bytes()).hexdigest() != SOURCE_SHA256:
        raise SystemExit("official source SHA-256 mismatch")
    results = []
    with tempfile.TemporaryDirectory(prefix="igrf14-reference-") as directory:
        binary = Path(directory) / "igrf14"
        subprocess.run(
            ["gfortran", "-std=legacy", "-O0", str(source), "-o", str(binary)],
            check=True,
        )
        for year, month, day, date, lat, lon, alt in CASES:
            # screen output, geodetic, spot, decimal degrees, date, km, lat/lon,
            # place name, then no further point. Upstream computes MF and SV.
            answers = f"\n1\n1\n2\n{date}\n{alt}\n{lat} {lon}\nR02\nn\n"
            output = subprocess.run(
                [str(binary)], input=answers, text=True, capture_output=True,
                check=True, cwd=directory, timeout=10,
            ).stdout
            components = {}
            for label in ("X", "Y", "Z", "F"):
                match = re.search(rf"\b{label} =\s*(-?\d+) nT", output)
                if match is None:
                    raise RuntimeError(f"missing {label}: {output}")
                components[label] = int(match[1])
            angles = {}
            for label in ("D", "I"):
                match = re.search(rf"\b{label} =\s*(-?\d+) deg\s*(-?\d+) min", output)
                if match is None:
                    raise RuntimeError(f"missing {label}: {output}")
                degrees, minutes = int(match[1]), int(match[2])
                angles[label] = degrees + (-minutes if degrees < 0 else minutes) / 60
            results.append(dict(
                year=year, month=month, day=day, decimal_year=date,
                latitude_deg=lat, longitude_deg=lon, altitude_km=alt,
                north_nt=components["X"], east_nt=components["Y"],
                down_nt=components["Z"], magnitude_nt=components["F"],
                declination_deg=angles["D"], inclination_deg=angles["I"],
            ))
    print(json.dumps(results, indent=2))
    if results != json.loads(FIXTURE.read_text()):
        raise SystemExit("reference fixture differs from official output")
    print(f"verified {len(results)} official reference cases", file=sys.stderr)


if __name__ == "__main__":
    main()

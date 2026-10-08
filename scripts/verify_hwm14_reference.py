"""Replay the unmodified pinned official driver, offline and without Rust.

Only the height and latitude profiles are checked (36 points, Quiet/total).
The committed expectations come from upstream Check/gfortran.txt, not this run.
"""

import argparse
import hashlib
from pathlib import Path
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[1] / "crates/models/hwm"
ARCHIVE_SHA256 = "4de451beeadef7b3ec3aa5b91129ea98866b9e7156cecf4be1343c33a6f57978"
REFERENCE_SHA256 = "2b1d4f4f103be3531393c48bf32d034548babfb6c080549884cad9fd3a2c8652"


def profiles(text):
    return text.split(" local time profile")[0]


def rows(text):
    result = []
    for line in profiles(text).splitlines():
        fields = line.split()
        if len(fields) == 7:
            try:
                result.append([float(field) for field in fields])
            except ValueError:
                pass
    if len(result) != 36:
        raise ValueError(f"expected 36 profile rows, got {len(result)}")
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--archive", type=Path, help="local pinned official archive")
    args = parser.parse_args()
    archive = args.archive or ROOT / "cache/hwm14.tgz"
    if not archive.is_file():
        raise SystemExit(f"Missing {archive}; provide --archive with the fixed official release (docs/releases.md)")
    if hashlib.sha256(archive.read_bytes()).hexdigest() != ARCHIVE_SHA256:
        raise SystemExit("official archive SHA-256 mismatch")
    with tempfile.TemporaryDirectory(prefix="hwm14-reference-") as directory:
        work = Path(directory)
        with tarfile.open(archive) as source:
            # Extract only the exact files used, with fixed destination names.
            for name in ["hwm14.f90", "checkhwm14.f90", "hwm123114.bin",
                         "dwm07b104i.dat", "gd2qd.dat"]:
                (work / name).write_bytes(source.extractfile(f"HWM14/{name}").read())
            reference = source.extractfile("HWM14/Check/gfortran.txt").read()
        if hashlib.sha256(reference).hexdigest() != REFERENCE_SHA256:
            raise SystemExit("official reference SHA-256 mismatch")
        fixture = (ROOT / "data/reference-profiles.txt").read_text()
        if fixture != profiles(reference.decode()):
            raise SystemExit("fixture is not the verbatim official profile excerpt")
        subprocess.run(["gfortran", "-O0", "hwm14.f90", "checkhwm14.f90",
                        "-o", "checkhwm14"], cwd=work, check=True, capture_output=True)
        actual = subprocess.run([str(work / "checkhwm14")], cwd=work,
                                check=True, capture_output=True, text=True,
                                timeout=30).stdout
        maximum = 0.0
        for index, (got, expected) in enumerate(zip(rows(actual), rows(fixture), strict=True)):
            if got[0] != expected[0]:
                raise SystemExit(f"row {index}: profile coordinate mismatch")
            for column, (a, b) in enumerate(zip(got[1:], expected[1:], strict=True), 1):
                error = abs(a - b)
                maximum = max(maximum, error)
                if error > 0.002:
                    raise SystemExit(f"row {index} column {column}: {a} != {b}")
        print(f"verified 36 official profile points; max error {maximum:.6f} m/s")


if __name__ == "__main__":
    main()

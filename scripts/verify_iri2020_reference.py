"""Replay the pinned point using unmodified upstream Fortran, not the Rust shim.

Run through nix develop .#default --command python3 scripts/verify_iri2020_reference.py.
The reference uses upstream's missing-Ap branch, selected through synthetic
unavailable-index sentinels in its COMMON block, without modifying any source.
This checks one switch
configuration, not all IRI options or the model's observational accuracy.
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
SHA256 = "3d1ab8c6e37ec2bf80a805264a2d6996d6cebf6cbab6369b8f329f0ef287d2f8"
DRIVER = """
program reference
  implicit none
  logical :: jf(50)
  real :: outf(20,1000), oarr(100)
  integer :: aap(27000,9), n
  real :: af107(27000,3)
  common /apfa/ aap, af107, n
  ! irisub.for switch table: URSI, IRI-cor2, Shubin, explicit indices,
  ! absolute ion densities; no storm, drift, spread-F, aurora or Es.
  jf = .true.
  jf([4,5,6,17,21,22,23,25,26,27,28,30,32,33,34,35,39,40,45,47]) = .false.
  outf = -1.0
  oarr = -1.0
  oarr(33) = 10.0
  oarr(39) = 10.0
  oarr(41) = 70.0
  oarr(46) = 70.0
  ! Synthetic missing Ap, NOT observations and NOT Ap=0. Original APF_ONLY
  ! initializes ISDATE; APFMSIS returns negative current Ap, selecting
  ! IRI_SUB's SWMI(9)=0 branch. All flux inputs are explicitly overridden.
  aap = -5
  af107 = -11.1
  n = 27000
  call iri_sub(jf,0,0.0,0.0,2020,320,37.0,300.0,300.0,1.0,outf,oarr)
  write(*,'(A,20ES26.17)') 'REFERENCE ',outf(1:11,1),oarr(1:6),oarr(23),oarr(25),oarr(27)
end program
"""


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--archive", type=Path, help="local pinned official archive")
    args = parser.parse_args()
    archive = args.archive or ROOT / "crates/models/iri/cache/iri2020.tar"
    if not archive.is_file():
        raise SystemExit(f"Missing {archive}; provide --archive with the pinned official release")
    for path, expected in [(archive, SHA256)]:
        if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise SystemExit(f"SHA-256 mismatch: {path}")
    with tempfile.TemporaryDirectory(prefix="iri2020-reference-") as directory:
        work = Path(directory)
        with tarfile.open(archive) as source:
            for member in source.getmembers():
                if member.isfile() and Path(member.name).parent == Path("IRI-zip"):
                    (work / Path(member.name).name).write_bytes(source.extractfile(member).read())
        (work / "reference.f90").write_text(DRIVER)
        sources = ["cira", "igrf", "iridreg", "iriflip", "irifun", "irisub", "iritec", "rocdrift"]
        command = ["gfortran", "-O2", "-std=legacy", "-ffixed-line-length-none",
                   "-fallow-argument-mismatch", *[f"{name}.for" for name in sources],
                   "reference.f90", "-o", "reference"]
        if sys.platform == "darwin":
            command.append("-fno-omit-frame-pointer")
        subprocess.run(command, cwd=work, check=True, capture_output=True)
        output = subprocess.run([str(work / "reference")], cwd=work, check=True,
                                capture_output=True, text=True, timeout=30).stdout
        line, = [line for line in output.splitlines() if line.startswith("REFERENCE ")]
        values = [float(value) for value in line.split()[1:]]
        expected = json.loads((ROOT / "crates/models/iri/data/reference-no-ap.json").read_text())
        for actual, target in zip(values, expected, strict=True):
            if not abs(actual - target) <= max(abs(target), 1.0) * 1e-6:
                raise SystemExit(f"reference mismatch: {actual} != {target}")
        print("Verified 20 fields against unmodified pinned IRI_SUB, internal magnetic response disabled")
        print(values)


if __name__ == "__main__":
    main()

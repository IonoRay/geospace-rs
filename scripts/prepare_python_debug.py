"""Rebuild the local extension offline using the existing Nix/maturin/venv only.

Run: nix develop .#default --command python3 scripts/prepare_python_debug.py
"""
import os
from pathlib import Path
import subprocess


def main():
    root = Path(__file__).resolve().parents[1]
    venv = root / ".venv"
    interpreter = venv / "bin/python"
    if not interpreter.is_file():
        raise SystemExit(f"Missing existing interpreter {interpreter}; no environment was installed.")
    env = dict(os.environ, IONORAY_OFFLINE="1", VIRTUAL_ENV=str(venv),
               PYO3_PYTHON=str(interpreter), IONORAY_IRI2020_OFFLINE="1",
               IONORAY_HWM14_OFFLINE="1", IONORAY_NRLMSIS21_OFFLINE="1")
    env["PATH"] = str(venv / "bin") + os.pathsep + env["PATH"]
    subprocess.run(["maturin", "develop", "--locked", "--offline"], check=True, cwd=root, env=env)
    subprocess.run([str(interpreter), "-c",
                    "import ionoray_geospace as gs; assert hasattr(gs, 'Session'); assert not hasattr(gs, 'point')"],
                   check=True, cwd=root, env=env)


if __name__ == "__main__":
    main()

"""Build bindings with explicit optional models, using the existing Nix/maturin/venv.
Run: nix develop .#default --command python3 scripts/prepare_python_debug.py --features standard
"""
import argparse
import os
from pathlib import Path
import subprocess

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--features", default="", help="comma-separated igrf,iri,hwm,msis,standard")
    parser.add_argument("--allow-model-network", action="store_true", help="permit fixed build assets to be acquired; Cargo dependencies remain offline")
    args = parser.parse_args()
    selected = set(filter(None, args.features.split(",")))
    if selected - {"igrf", "iri", "hwm", "msis", "standard"}:
        parser.error("unknown model feature")
    root = Path(__file__).resolve().parents[1]
    venv = root / ".venv"
    interpreter = venv / "bin/python"
    if not interpreter.is_file():
        raise SystemExit(f"Missing interpreter {interpreter}; create the documented venv first.")
    env = dict(os.environ, VIRTUAL_ENV=str(venv), PYO3_PYTHON=str(interpreter))
    if not args.allow_model_network:
        env["IONORAY_OFFLINE"] = "1"
    env["PATH"] = str(venv / "bin") + os.pathsep + env["PATH"]
    features = ",".join(["extension-module", *sorted(selected)])
    subprocess.run(["maturin", "develop", "--locked", "--offline", "--no-default-features", "--features", features], check=True, cwd=root, env=env)
    subprocess.run([str(interpreter), "-c", "import ionoray_geospace as gs; print(gs.capabilities()); assert hasattr(gs, 'Session')"], check=True, cwd=root, env=env)

if __name__ == "__main__":
    main()

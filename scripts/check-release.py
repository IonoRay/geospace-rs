#!/usr/bin/env python3
"""Bounded local release audit: history, branch scope, assets, licenses and Cargo lists.
Run through Nix. No commits, downloads, registry publishing or remote updates.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('sync_cache', ROOT / 'scripts/sync-cache.py')
cache = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cache)

def git(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT)

def audit_branches(main, snapshots):
    paths = set(cache.ASSET_GROUPS)
    ancestor = git('merge-base', main, snapshots).decode().strip()
    assert ancestor == git('rev-parse', main).decode().strip(), 'cache-snapshots must contain main'
    for commit in git('rev-list', main).decode().splitlines():
        tracked = set(git('ls-tree', '-r', '--name-only', commit).decode().splitlines())
        assert not paths & tracked, f'main history contains snapshots at {commit}'
    diff = set(git('diff', '--name-only', main, snapshots).decode().splitlines())
    assert diff == paths, f'branch differences must be exactly the eight snapshots: {diff ^ paths}'
    manifest = json.loads(git('show', f'{main}:{cache.MANIFEST_PATH}'))
    for entry in cache.validate_manifest(manifest):
        body = git('show', f'{snapshots}:{entry["path"]}')
        assert len(body) == entry['size'] and hashlib.sha256(body).hexdigest() == entry['sha256'], entry['path']
    print(f'branches: shared base {ancestor}; main history clean; exactly {len(diff)} snapshot differences')

def audit_files():
    for path in (ROOT / 'crates').rglob('*.rs'):
        if path.name.endswith('tests.rs') or 'tests' in path.parts:
            continue
        assert len(path.read_text().splitlines()) < 500, f'production Rust file >=500 lines: {path}'
    for cargo in (ROOT / 'crates').rglob('Cargo.toml'):
        package = tomllib.loads(cargo.read_text())['package']
        for license in ['LICENSE-MIT', 'LICENSE-APACHE']:
            assert (cargo.parent / license).read_bytes() == (ROOT / license).read_bytes(), cargo
        assert (cargo.parent / 'THIRD_PARTY_NOTICES.md').is_file(), cargo
        if 'license-file' in package:
            assert (cargo.parent / package['license-file']).is_file(), cargo
    assert tomllib.loads((ROOT / 'crates/models/hwm/Cargo.toml').read_text())['package']['publish'] is False
    assert tomllib.loads((ROOT / 'crates/models/msis/Cargo.toml').read_text())['package']['publish'] is False
    print('files: production size limits and per-crate original/upstream license scope verified')

def audit_packages():
    for cargo in (ROOT / 'crates').rglob('Cargo.toml'):
        package = tomllib.loads(cargo.read_text())['package']['name']
        result = subprocess.run(['cargo', 'package', '--locked', '--offline', '-p', package, '--allow-dirty', '--list'], cwd=ROOT, text=True, capture_output=True)
        if result.returncode:
            raise RuntimeError(f'{package} package listing failed: {result.stderr}')
        files = set(result.stdout.splitlines())
        assert {'LICENSE-MIT', 'LICENSE-APACHE', 'THIRD_PARTY_NOTICES.md'} <= files, package
        for path in cache.ASSET_GROUPS:
            try:
                relative = (ROOT / path).relative_to(cargo.parent).as_posix()
            except ValueError:
                continue
            assert relative not in files, f'{package} leaks source snapshot: {relative}'
        for path in ['LICENSE','IRI-LICENSE.txt','nrlmsis2.1_license.txt','GFZ-NOTICE.txt']:
            if (cargo.parent / path).exists():
                assert path in files, f'{package} omits license {path}'
        print(f'package: {package}, {len(files)} files, licenses included, no source snapshots')

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--main', default='main')
    parser.add_argument('--snapshots', default='cache-snapshots')
    parser.add_argument('--skip-branches', action='store_true', help='audit the implementation before branch creation')
    parser.add_argument('--packages', action='store_true', help='check every actual Cargo package file list')
    args = parser.parse_args()
    audit_files()
    if not args.skip_branches:
        audit_branches(args.main, args.snapshots)
    if args.packages:
        audit_packages()
    print('Engineering audit passed. Public release still requires HWM/MSIS rights evidence; see docs/releases.md.')

if __name__ == '__main__':
    main()

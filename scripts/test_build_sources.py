"""Real fixed assets + fresh OUT_DIR tests of the compiled Cargo build scripts.
Run inside Nix after a full-model build, with --cache-root an approved snapshot
root and --target the Cargo target directory. No network/dependency downloads.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
MODELS = {
    'hwm': ('HWM14', 'hwm14', 'cache/hwm14.tgz', 'crates/models/hwm/cache/hwm14.tgz'),
    'msis': ('NRLMSIS21', 'nrlmsis21', 'cache/nrlmsis2.1.tar.gz', 'crates/models/msis/cache/nrlmsis2.1.tar.gz'),
    'iri': ('IRI2020', 'iri2020', 'cache/iri2020.tar', 'crates/models/iri/cache/iri2020.tar'),
    'igrf': ('IGRF14', None, 'data/igrf14coeffs.txt', 'crates/models/igrf/data/igrf14coeffs.txt'),
}

class BuildSourceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='ionoray-build-source-test-')
        self.addCleanup(self.temp.cleanup)
        self.work = Path(self.temp.name)

    def run_build(self, model, root=None):
        identity, feature, local, relative = MODELS[model]
        env = dict(os.environ)
        for name in list(env):
            if name.startswith('IONORAY_') or name.startswith('CARGO_FEATURE_'):
                env.pop(name)
        manifest = self.work / model / 'manifest'
        out = self.work / model / 'out'
        manifest.mkdir(parents=True, exist_ok=True)
        out.mkdir(parents=True, exist_ok=True)
        env.update(CARGO_MANIFEST_DIR=str(manifest), OUT_DIR=str(out),
                   IONORAY_OFFLINE='1', CARGO_CFG_TARGET_OS='macos' if os.uname().sysname=='Darwin' else 'linux',
                   CARGO_CFG_TARGET_VENDOR='apple' if os.uname().sysname=='Darwin' else 'unknown')
        if feature:
            env['CARGO_FEATURE_'+feature.upper()] = '1'
        if root:
            env['IONORAY_CACHE_ROOT'] = str(root)
        matches = sorted(ARGS.target.glob(f'debug/build/ionoray-{model}-*/build-script-build'), key=lambda p: p.stat().st_mtime_ns)
        self.assertTrue(matches, f'Build ionoray-{model} in {ARGS.target} first')
        return subprocess.run([str(matches[-1])], cwd=manifest, env=env, text=True, capture_output=True, timeout=120)

    def test_fresh_offline_builds_require_real_sources(self):
        for model in MODELS:
            with self.subTest(model=model):
                result = self.run_build(model)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('offline build has no', result.stderr)

    def test_corrupted_selected_snapshots_fail_before_using_out_dir(self):
        for model, (_, _, local, _) in MODELS.items():
            with self.subTest(model=model):
                file = self.work / model / 'manifest' / local
                file.parent.mkdir(parents=True, exist_ok=True)
                file.write_bytes(b'explicit synthetic corrupted snapshot\n')
                result = self.run_build(model)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('SHA-256 mismatch', result.stderr)

    def test_external_cache_and_cached_out_dir_both_verify(self):
        for model in MODELS:
            with self.subTest(model=model):
                result = self.run_build(model, ARGS.cache_root)
                self.assertEqual(result.returncode, 0, result.stderr)
                again = self.run_build(model)
                self.assertEqual(again.returncode, 0, again.stderr)
                _, _, local, _ = MODELS[model]
                file = self.work / model / 'manifest' / local
                file.parent.mkdir(parents=True, exist_ok=True)
                file.write_bytes(b'synthetic corruption while OUT_DIR remains valid\n')
                changed = self.run_build(model)
                self.assertNotEqual(changed.returncode, 0)
                self.assertIn('SHA-256 mismatch', changed.stderr)

    def test_indices_missing_some_and_corruption_generate_current_optional_state(self):
        # Exercise the production build script with a fresh output and a mixed root.
        matches = sorted(ARGS.target.glob('debug/build/ionoray-indices-*/build-script-build'), key=lambda p:p.stat().st_mtime_ns)
        self.assertTrue(matches)
        manifest = self.work/'indices'; manifest.mkdir()
        out = self.work/'index-out';out.mkdir()
        env = dict(os.environ, CARGO_MANIFEST_DIR=str(manifest), OUT_DIR=str(out))
        env.pop('IONORAY_CACHE_ROOT', None)
        def run(): return subprocess.run([str(matches[-1])], cwd=manifest, env=env, text=True, capture_output=True, timeout=30)
        self.assertEqual(run().returncode, 0)
        self.assertEqual((out/'optional_cache.rs').read_text().count(' = None;'), 3)
        env['IONORAY_CACHE_ROOT']=str(ARGS.cache_root)
        self.assertEqual(run().returncode, 0)
        self.assertEqual((out/'optional_cache.rs').read_text().count('Some(include_bytes!'), 3)
        env.pop('IONORAY_CACHE_ROOT')
        self.assertEqual(run().returncode, 0)
        self.assertEqual((out/'optional_cache.rs').read_text().count(' = None;'), 3)
        file=manifest/'cache/ig_rz.dat';file.parent.mkdir();file.write_bytes(b'bad synthetic cache')
        failed=run()
        self.assertNotEqual(failed.returncode,0)
        self.assertIn('SHA-256 mismatch',failed.stderr)

if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cache-root',type=Path,required=True)
    parser.add_argument('--target',type=Path,default=ROOT/'target')
    ARGS, remaining=parser.parse_known_args()
    ARGS.cache_root=ARGS.cache_root.resolve();ARGS.target=ARGS.target.resolve()
    unittest.main(argv=['test_build_sources.py',*remaining])

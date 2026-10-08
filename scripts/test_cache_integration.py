"""Real bytes via bounded local HTTP; neither GitHub nor upstream is contacted.
Requires an already approved snapshot root. No dependency/model downloads.
"""
import argparse
import contextlib
import hashlib
import io
import json
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_sync_cache import CacheSyncTests, sync_cache
from test_update_cache import update


class RealCacheIntegrationTests(CacheSyncTests):
    # Reuse HTTP setup, without inheriting the synthetic control-flow test cases.
    def test_real_single_file_download_sync_and_reuse(self):
        root = Path(__file__).resolve().parents[1]
        self.manifest = json.loads((root / sync_cache.MANIFEST_PATH).read_text())
        self.payloads = {entry['path']: ((root if entry['group'] == 'notices' else ARGS.cache_root) / entry['path']).read_bytes()
                         for entry in self.manifest['files']}
        script = Path(self.temp.name) / 'standalone/sync-cache.py'
        script.parent.mkdir()
        self.payloads['scripts/sync-cache.py'] = (root/'scripts/sync-cache.py').read_bytes()
        base = f'http://127.0.0.1:{self.server.server_port}'
        subprocess.run(['curl','--fail','--silent','--show-error','--max-time','10',
                        base+self.prefix+'scripts/sync-cache.py','--output',str(script)],check=True)
        # The downloaded tool imports independently, with local fixture endpoints.
        import importlib.util
        spec = importlib.util.spec_from_file_location('standalone',script)
        standalone=importlib.util.module_from_spec(spec);spec.loader.exec_module(standalone)
        with patch.object(standalone,'API',base),patch.object(standalone,'RAW',base),contextlib.redirect_stdout(io.StringIO()):
            args=argparse.Namespace(repo='IonoRay/geospace-rs',ref='cache-snapshots',dest=str(self.root),manifest=None,only=None,dry_run=False)
            result=standalone.sync(args)
            again=standalone.sync(args)
        self.assertEqual(len(result['downloaded']),16)
        self.assertEqual(again['downloaded'],[])
        for entry in self.manifest['files']:
            data=(self.root/entry['path']).read_bytes()
            self.assertEqual(len(data),entry['size'])
            self.assertEqual(hashlib.sha256(data).hexdigest(),entry['sha256'])
        receipt=json.loads((self.root/standalone.RECEIPT_PATH).read_text())
        self.assertEqual(receipt['commit'],result['commit'])

    def test_full_preparation_reuses_real_assets_and_generates_matching_manifest(self):
        args=argparse.Namespace(dest=self.root,indices_only=False,check=False,no_verify=True,transport='local',remote_host='')
        def download(source,output,*args):
            output.write_bytes((ARGS.cache_root/source.destination.relative_to(update.ROOT)).read_bytes())
            return 'real-local-input'
        with patch.object(update,'parse_args',return_value=args),patch.object(update,'download',side_effect=download),contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(update.main(),0)
        manifest=json.loads((self.root/'assets/cache-manifest.json').read_text())
        self.assertEqual(len(sync_cache.validate_manifest(manifest)),16)
        for entry in manifest['files']:
            self.assertEqual(hashlib.sha256((self.root/entry['path']).read_bytes()).hexdigest(),entry['sha256'])
        self.assertEqual((self.root/'crates/models/hwm/data/reference-profiles.txt').read_bytes(),
                         (ARGS.cache_root/'crates/models/hwm/data/reference-profiles.txt').read_bytes())


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cache-root',type=Path,required=True)
    ARGS=parser.parse_args();ARGS.cache_root=ARGS.cache_root.resolve()
    suite=unittest.TestSuite(RealCacheIntegrationTests(name) for name in [
        'test_real_single_file_download_sync_and_reuse',
        'test_full_preparation_reuses_real_assets_and_generates_matching_manifest'])
    sys.exit(not unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful())

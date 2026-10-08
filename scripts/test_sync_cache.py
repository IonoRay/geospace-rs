"""Synthetic, bounded HTTP fixtures; no GitHub/upstream downloads."""
from argparse import Namespace
import contextlib
import hashlib
from http.server import BaseHTTPRequestHandler, HTTPServer
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("sync_cache", Path(__file__).with_name("sync-cache.py"))
sync_cache = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sync_cache)
COMMIT = "a" * 40


class CacheSyncTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="ionoray-cache-sync-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "destination"
        paths = {**sync_cache.ASSET_GROUPS, **dict.fromkeys(sync_cache.NOTICE_PATHS, "notices")}
        self.payloads = {path: f"synthetic fixture for {path}\n".encode() for path in sorted(paths)}
        self.manifest = {
            "schema_version": 1, "repository": "IonoRay/geospace-rs", "snapshot_id": "synthetic-test",
            "files": [{"path": path, "group": paths[path], "size": len(data),
                       "sha256": hashlib.sha256(data).hexdigest()} for path, data in self.payloads.items()],
        }
        self.requests = []
        self.auth_headers = []
        self.commit_path = "/repos/IonoRay/geospace-rs/commits/cache-snapshots"
        self.prefix = f"/IonoRay/geospace-rs/{COMMIT}/"
        owner = self

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                owner.requests.append(self.path)
                owner.auth_headers.append(self.headers.get("Authorization"))
                if self.path == owner.commit_path:
                    data = json.dumps({"sha": COMMIT}).encode()
                elif self.path.startswith("/repos/IonoRay/geospace-rs/contents/"):
                    path, ref = self.path.removeprefix("/repos/IonoRay/geospace-rs/contents/").split("?ref=")
                    if ref != COMMIT:
                        self.send_error(404)
                        return
                    data = json.dumps(owner.manifest).encode() if path == sync_cache.MANIFEST_PATH else owner.payloads.get(path)
                    if data is None:
                        self.send_error(404)
                        return
                elif self.path == owner.prefix + sync_cache.MANIFEST_PATH:
                    data = json.dumps(owner.manifest).encode()
                elif self.path.startswith(owner.prefix):
                    data = owner.payloads.get(self.path[len(owner.prefix):])
                    if data is None:
                        self.send_error(404)
                        return
                else:
                    self.send_error(404)
                    return
                self.send_response(200)
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def log_message(self, *args):
                pass

        self.server = HTTPServer(("127.0.0.1", 0), Handler)
        self.worker = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.worker.start()
        self.addCleanup(self.stop_server)
        base = f"http://127.0.0.1:{self.server.server_port}"
        self.patches = [patch.object(sync_cache, "API", base), patch.object(sync_cache, "RAW", base),
                        patch.dict("os.environ", {"GH_TOKEN": "", "GITHUB_TOKEN": ""})]
        for item in self.patches:
            item.start()
            self.addCleanup(item.stop)

    def stop_server(self):
        self.server.shutdown()
        self.server.server_close()
        self.worker.join()

    def run_sync(self, **overrides):
        args = Namespace(repo="IonoRay/geospace-rs", ref="cache-snapshots", dest=str(self.root),
                         manifest=None, only=None, dry_run=False)
        vars(args).update(overrides)
        with contextlib.redirect_stdout(io.StringIO()):
            return sync_cache.sync(args)

    def update_asset(self, path):
        self.payloads[path] = b"new synthetic asset\n"
        entry = next(e for e in self.manifest["files"] if e["path"] == path)
        entry.update(size=len(self.payloads[path]), sha256=hashlib.sha256(self.payloads[path]).hexdigest())

    def test_sync_pins_commit_and_reuses_verified_files(self):
        result = self.run_sync()
        self.assertEqual(result["commit"], COMMIT)
        for path, data in self.payloads.items():
            self.assertEqual((self.root / path).read_bytes(), data)
        receipt = json.loads((self.root / sync_cache.RECEIPT_PATH).read_bytes())
        self.assertEqual(receipt["commit"], COMMIT)
        before = len(self.requests)
        again = self.run_sync()
        self.assertEqual(again["downloaded"], [])
        self.assertEqual(len(self.requests) - before, 2)
        self.assertTrue(all(COMMIT in path for path in self.requests if path != self.commit_path))

    def test_dry_run_does_not_download_assets_or_create_destination(self):
        self.run_sync(dry_run=True)
        self.assertFalse(self.root.exists())
        self.assertEqual(len(self.requests), 2)

    def test_hash_mismatch_does_not_install_any_asset(self):
        self.payloads[next(iter(self.payloads))] = b"bad"
        with self.assertRaisesRegex(sync_cache.SyncError, "mismatch"):
            self.run_sync()
        self.assertFalse(any(self.root.rglob("*.tgz")))
        self.assertFalse((self.root / "LICENSE").exists())
        self.assertFalse((self.root / sync_cache.RECEIPT_PATH).exists())

    def test_missing_remote_asset_leaves_existing_files_untouched(self):
        del self.payloads["crates/models/msis/cache/nrlmsis2.1.tar.gz"]
        with self.assertRaisesRegex(sync_cache.SyncError, "HTTP 404"):
            self.run_sync()
        self.assertFalse((self.root / "LICENSE").exists())

    def test_local_changes_are_preserved(self):
        self.run_sync()
        target = self.root / "crates/models/igrf/data/igrf14coeffs.txt"
        target.write_bytes(b"local user changes")
        with self.assertRaisesRegex(sync_cache.SyncError, "preserved"):
            self.run_sync()
        self.assertEqual(target.read_bytes(), b"local user changes")

    def test_unmanaged_existing_file_is_preserved(self):
        self.root.mkdir()
        (self.root / "LICENSE").write_bytes(b"user-owned file")
        with self.assertRaisesRegex(sync_cache.SyncError, "preserved"):
            self.run_sync()
        self.assertEqual((self.root / "LICENSE").read_bytes(), b"user-owned file")

    def test_managed_snapshot_updates_without_touching_other_files(self):
        self.run_sync()
        asset = "crates/models/igrf/data/igrf14coeffs.txt"
        self.update_asset(asset)
        result = self.run_sync()
        self.assertEqual(result["downloaded"], [asset])
        self.assertEqual((self.root / asset).read_bytes(), self.payloads[asset])

    def test_main_manifest_mismatch_rejects_other_snapshot(self):
        self.root.mkdir()
        local = self.root / sync_cache.MANIFEST_PATH
        local.parent.mkdir()
        local.write_text(json.dumps(self.manifest))
        self.update_asset("crates/models/igrf/data/igrf14coeffs.txt")
        with self.assertRaisesRegex(sync_cache.SyncError, "matching --ref"):
            self.run_sync()
        self.assertFalse((self.root / "LICENSE").exists())

    def test_selective_sync_always_includes_notices(self):
        result = self.run_sync(only=["igrf"])
        self.assertIn("LICENSE", result["downloaded"])
        self.assertTrue((self.root / "crates/models/igrf/data/igrf14coeffs.txt").exists())
        self.assertFalse((self.root / "crates/models/hwm/cache/hwm14.tgz").exists())

    def test_symlink_parent_is_rejected(self):
        self.root.mkdir()
        outside = Path(self.temp.name) / "outside"
        outside.mkdir()
        (self.root / "crates").symlink_to(outside, target_is_directory=True)
        with self.assertRaisesRegex(sync_cache.SyncError, "symlink"):
            self.run_sync()
        self.assertEqual(list(outside.iterdir()), [])

    def test_invalid_manifest_paths_and_duplicate_entries_are_rejected(self):
        for path in ["../outside", "/outside", "crates/../outside", "src/lib.rs"]:
            changed = json.loads(json.dumps(self.manifest))
            changed["files"][0]["path"] = path
            with self.assertRaises(sync_cache.SyncError):
                sync_cache.validate_manifest(changed, "IonoRay/geospace-rs")
        self.manifest["files"].append(self.manifest["files"][0])
        with self.assertRaises(sync_cache.SyncError):
            self.run_sync()

    def test_authenticated_requests_use_api_and_same_pinned_commit(self):
        with patch.dict("os.environ", {"GH_TOKEN": "synthetic-test-token"}):
            self.run_sync(only=["igrf"])
        self.assertTrue(all(path.startswith("/repos/") for path in self.requests))
        self.assertTrue(all(header == "Bearer synthetic-test-token" for header in self.auth_headers))
        self.assertTrue(all(path.endswith("?ref=" + COMMIT) for path in self.requests[1:]))


if __name__ == "__main__":
    unittest.main()

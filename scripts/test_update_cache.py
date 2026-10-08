"""Synthetic calendar fixtures test preparation boundaries, not scientific values."""
from argparse import Namespace
import contextlib
import datetime as dt
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("update_cache", Path(__file__).with_name("update-cache.py"))
update = importlib.util.module_from_spec(spec)
spec.loader.exec_module(update)


def synthetic_indices():
    dates = [dt.date(2024, 1, 1) + dt.timedelta(days=i) for i in range(366)]
    gfz = "".join(f"{d.year} {d.month} {d.day}\n" for d in dates).encode()
    ap = "".join(f"{d.year % 100:3}{d.month:3}{d.day:3}" + " " * 30 + "100.0" + " " * 10 + "\n" for d in dates).encode()
    ig = (",".join(map(str, [1, 1, 2024, 1, 2024, 12, 2024] + [100] * 28)) + ",").encode()
    return dict(zip((source.name for source in update.INDEX_SOURCES), (gfz, ig, ap)))


class PreparationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="ionoray-update-test-")
        self.addCleanup(self.temp.cleanup)
        self.destination = Path(self.temp.name) / "output"
        self.payloads = synthetic_indices()
        self.args = Namespace(dest=self.destination, indices_only=True, check=False,
                              no_verify=True, transport="local", remote_host="")

    def run_prepare(self):
        def download(source, output, *args):
            output.write_bytes(self.payloads[source.name])
            return "synthetic-local-fixture"
        with patch.object(update, "parse_args", return_value=self.args), \
                patch.object(update, "download", side_effect=download), \
                contextlib.redirect_stdout(io.StringIO()):
            return update.main()

    def test_prepares_metadata_without_editing_checkout(self):
        before = {p: p.read_bytes() for p in [update.MANIFEST, update.ROOT_README, update.INDEX_README]}
        self.assertEqual(self.run_prepare(), 0)
        for path, data in before.items():
            self.assertEqual(path.read_bytes(), data)
        prepared = self.destination / update.MANIFEST.relative_to(update.ROOT)
        self.assertIn('GFZ_END_YEAR: u16 = 2024', prepared.read_text())
        self.assertFalse((self.destination / "assets/cache-manifest.json").exists())

    def test_check_creates_no_output(self):
        self.args.check = True
        self.assertEqual(self.run_prepare(), 0)
        self.assertFalse(self.destination.exists())

    def test_output_inside_checkout_is_rejected_before_download(self):
        self.args.dest = update.ROOT / "target/update-test"
        with self.assertRaisesRegex(update.UpdateError, "outside the source"):
            self.run_prepare()

    def test_bad_calendar_or_stale_manifest_preserves_existing_output(self):
        self.destination.mkdir()
        marker = self.destination / "user.txt"
        marker.write_bytes(b"user-owned")
        self.payloads[next(iter(self.payloads))] = b"2024 1 1\n"
        with self.assertRaisesRegex(update.UpdateError, "no complete calendar"):
            self.run_prepare()
        self.assertEqual(list(self.destination.iterdir()), [marker])
        self.payloads = synthetic_indices()
        manifest = self.destination / "assets/cache-manifest.json"
        manifest.parent.mkdir()
        manifest.write_text(json.dumps({"existing": "manifest"}))
        with self.assertRaisesRegex(update.UpdateError, "stale shared manifest"):
            self.run_prepare()
        self.assertEqual(marker.read_bytes(), b"user-owned")
        self.assertEqual(json.loads(manifest.read_text()), {"existing": "manifest"})

    def test_failed_verification_restores_all_written_files(self):
        old = self.destination / update.INDEX_SOURCES[0].destination.relative_to(update.ROOT)
        old.parent.mkdir(parents=True)
        old.write_bytes(b"prior output")
        self.args.no_verify = False
        with patch.object(update, "verify", side_effect=update.UpdateError("synthetic verification failure")):
            with self.assertRaisesRegex(update.UpdateError, "verification failure"):
                self.run_prepare()
        self.assertEqual(old.read_bytes(), b"prior output")
        self.assertEqual([p for p in self.destination.rglob('*') if p.is_file()], [old])


if __name__ == "__main__":
    unittest.main()

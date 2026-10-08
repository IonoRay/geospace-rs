#!/usr/bin/env python3
"""Refresh bundled geospace caches from their authoritative upstreams."""

from __future__ import annotations

import argparse
import calendar
import datetime as dt
import hashlib
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tempfile
import uuid


ROOT = Path(__file__).resolve().parents[1]
INDEX_CACHE = ROOT / "crates/indices/cache"
MANIFEST = ROOT / "crates/indices/src/download/cache_manifest.rs"
ROOT_README = ROOT / "README.md"
INDEX_README = INDEX_CACHE / "README.md"
USER_AGENT = "ionoray-cache-update/0.1.0"


class UpdateError(RuntimeError):
    pass


class Source:
    def __init__(
        self,
        name: str,
        url: str,
        destination: Path,
        *,
        pinned_sha256: str | None = None,
        referer: str | None = None,
    ) -> None:
        self.name = name
        self.url = url
        self.destination = destination
        self.pinned_sha256 = pinned_sha256
        self.referer = referer


INDEX_SOURCES = (
    Source(
        "GFZ Kp/ap/Ap/SN/F10.7",
        "https://kp.gfz.de/fileadmin/files_for_gfz_cms/"
        "Kp_ap_Ap_SN_F107_since_1932.txt",
        INDEX_CACHE / "Kp_ap_Ap_SN_F107_since_1932.txt",
    ),
    Source(
        "IRI IG/Rz",
        "https://irimodel.org/indices/ig_rz.dat",
        INDEX_CACHE / "ig_rz.dat",
        referer="https://irimodel.org/indices/",
    ),
    Source(
        "IRI AP/F10.7",
        "https://irimodel.org/indices/apf107.dat",
        INDEX_CACHE / "apf107.dat",
        referer="https://irimodel.org/indices/",
    ),
)

MODEL_SOURCES = (
    Source(
        "NRLMSIS 2.1",
        "https://map.nrl.navy.mil/map/pub/nrl/NRLMSIS/NRLMSIS2.1/"
        "nrlmsis2.1.tar.gz",
        ROOT / "crates/models/msis/cache/nrlmsis2.1.tar.gz",
        pinned_sha256="41e47b29f795d36a5cc252b2858aa2a384c4a7323ace3d48d3ea2f2b37a1a6a8",
    ),
    Source(
        "HWM14",
        "https://map.nrl.navy.mil/map/pub/nrl/HWM/HWM14/"
        "HWM14_ess224-sup-0002-supinfo.tgz",
        ROOT / "crates/models/hwm/cache/hwm14.tgz",
        pinned_sha256="4de451beeadef7b3ec3aa5b91129ea98866b9e7156cecf4be1343c33a6f57978",
    ),
    Source(
        "IRI-2020",
        "https://irimodel.org/IRI-2020/00_iri.tar",
        ROOT / "crates/models/iri/cache/iri2020.tar",
        pinned_sha256="3d1ab8c6e37ec2bf80a805264a2d6996d6cebf6cbab6369b8f329f0ef287d2f8",
        referer="https://irimodel.org/IRI-2020/",
    ),
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Download and validate every bundled cache, then update rolling "
            "index snapshots atomically. Run inside the default Nix dev shell."
        )
    )
    parser.add_argument(
        "--transport",
        choices=("auto", "local", "remote"),
        default="auto",
        help=(
            "download locally, remotely, or use an explicitly configured "
            "fallback (default: auto)"
        ),
    )
    parser.add_argument(
        "--remote-host",
        default="",
        help="optional SSH host used for remote download; empty by default",
    )
    parser.add_argument(
        "--indices-only",
        action="store_true",
        help="skip re-validating the pinned model archives",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="download, validate, and report without writing files",
    )
    parser.add_argument(
        "--no-verify",
        action="store_true",
        help="write validated caches without running the Rust verification gates",
    )
    args = parser.parse_args()
    args.remote_host = args.remote_host.strip()
    if args.transport == "remote" and not args.remote_host:
        parser.error("--transport remote requires --remote-host")
    return args


def run(command: list[str], *, env: dict[str, str] | None = None) -> None:
    shown = " ".join(shlex.quote(part) for part in command)
    print(f"+ {shown}", flush=True)
    result = subprocess.run(command, cwd=ROOT, env=env, check=False)
    if result.returncode != 0:
        raise UpdateError(f"command failed with exit code {result.returncode}: {shown}")


def run_with_retry(command: list[str], *, attempts: int = 2) -> None:
    for attempt in range(1, attempts + 1):
        try:
            run(command)
            return
        except UpdateError:
            if attempt == attempts:
                raise
            print(
                f"  transient command failure; retrying ({attempt + 1}/{attempts})",
                file=sys.stderr,
            )


def curl_command(source: Source, output: str) -> list[str]:
    command = [
        "curl",
        "--fail",
        "--location",
        "--silent",
        "--show-error",
        "--connect-timeout",
        "15",
        "--max-time",
        "90",
        "--speed-limit",
        "1024",
        "--speed-time",
        "30",
        "--retry",
        "1",
        "--retry-delay",
        "2",
        "--user-agent",
        USER_AGENT,
    ]
    if source.referer:
        command.extend(("--referer", source.referer))
    command.extend(("--output", output, source.url))
    return command


def download_local(source: Source, output: Path) -> None:
    run(curl_command(source, str(output)))


def download_remote(source: Source, output: Path, host: str) -> None:
    token = uuid.uuid4().hex
    remote_dir = f"/tmp/geospace-rs-cache-update-{token}"
    remote_file = f"{remote_dir}/payload"
    quoted_host = host.strip()
    if not quoted_host:
        raise UpdateError("remote download requires an explicit SSH host")
    if not re.fullmatch(r"[A-Za-z0-9_.@-]+", quoted_host):
        raise UpdateError(f"unsafe SSH host: {host!r}")
    mkdir = f"mkdir -p -- {shlex.quote(remote_dir)}"
    curl = " ".join(shlex.quote(part) for part in curl_command(source, remote_file))
    cleanup = f"rm -rf -- {shlex.quote(remote_dir)}"
    ssh = [
        "ssh",
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=15",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=2",
        quoted_host,
    ]
    try:
        run_with_retry([*ssh, f"{mkdir} && {curl}"])
        run_with_retry(
            [
                "rsync",
                "--archive",
                "--protect-args",
                "--",
                f"{quoted_host}:{remote_file}",
                str(output),
            ]
        )
    finally:
        subprocess.run(
            [*ssh, cleanup],
            cwd=ROOT,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )


def download(source: Source, output: Path, transport: str, host: str) -> str:
    if transport == "local":
        download_local(source, output)
        return "local"
    if transport == "remote":
        download_remote(source, output, host)
        return host
    try:
        download_local(source, output)
        return "local"
    except UpdateError as local_error:
        if not host:
            raise
        print(f"  local download failed: {local_error}", file=sys.stderr)
        print(f"  retrying through {host}", file=sys.stderr)
        download_remote(source, output, host)
        return host


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def complete_years(dates: set[dt.date]) -> tuple[int, int]:
    if not dates:
        raise UpdateError("source contains no valid dated records")
    years: list[int] = []
    for year in range(min(date.year for date in dates), max(date.year for date in dates) + 1):
        expected = 366 if calendar.isleap(year) else 365
        if sum(date.year == year for date in dates) == expected:
            years.append(year)
    if not years:
        raise UpdateError("source contains no complete calendar year")
    if years != list(range(years[0], years[-1] + 1)):
        raise UpdateError("complete calendar years are not contiguous")
    return years[0], years[-1]


def gfz_coverage(data: bytes) -> tuple[int, int]:
    try:
        text = data.decode("ascii")
    except UnicodeDecodeError as error:
        raise UpdateError("GFZ cache is not ASCII") from error
    dates: set[dt.date] = set()
    for line_number, line in enumerate(text.splitlines(), 1):
        if not line or line.startswith("#"):
            continue
        fields = line.split()
        if len(fields) < 3:
            raise UpdateError(f"GFZ line {line_number} has an incomplete date")
        try:
            date = dt.date(int(fields[0]), int(fields[1]), int(fields[2]))
        except (ValueError, OverflowError) as error:
            raise UpdateError(f"GFZ line {line_number} has an invalid date") from error
        if date in dates:
            raise UpdateError(f"GFZ contains duplicate date {date.isoformat()}")
        dates.add(date)
    return complete_years(dates)


def ig_rz_coverage(data: bytes) -> tuple[int, int]:
    try:
        fields = [field.strip() for field in data.decode("ascii").split(",") if field.strip()]
    except UnicodeDecodeError as error:
        raise UpdateError("IRI IG/Rz cache is not ASCII") from error
    if len(fields) < 9:
        raise UpdateError("IRI IG/Rz cache has an incomplete header")
    try:
        header = [int(field) for field in fields[:7]]
        for field in fields[7:]:
            float(field)
    except ValueError as error:
        raise UpdateError("IRI IG/Rz cache contains a non-numeric field") from error
    _, _, _, start_month, start_year, end_month, end_year = header
    if not 1 <= start_month <= 12 or not 1 <= end_month <= 12 or end_year < start_year:
        raise UpdateError("IRI IG/Rz cache has an invalid month range")
    month_count = (end_year * 12 + end_month - 1) - (start_year * 12 + start_month - 1) + 3
    expected_fields = 7 + month_count * 2
    if len(fields) != expected_fields:
        raise UpdateError(
            f"IRI IG/Rz cache contains {len(fields)} fields; expected {expected_fields}"
        )
    # The parser needs both adjacent months to interpolate an entire calendar year.
    full_end_year = end_year if end_month == 12 else end_year - 1
    if full_end_year < start_year:
        raise UpdateError("IRI IG/Rz cache contains no complete interpolation year")
    return start_year, full_end_year


def apf107_coverage(data: bytes) -> tuple[int, int]:
    try:
        text = data.decode("ascii")
    except UnicodeDecodeError as error:
        raise UpdateError("IRI AP/F10.7 cache is not ASCII") from error
    dates: set[dt.date] = set()
    for line_number, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        encoded = line.encode("ascii")
        if len(encoded) < 54:
            raise UpdateError(f"IRI AP/F10.7 line {line_number} is shorter than 54 bytes")
        try:
            raw_year = int(encoded[0:3])
            year = 1900 + raw_year if raw_year >= 58 else 2000 + raw_year
            date = dt.date(year, int(encoded[3:6]), int(encoded[6:9]))
            daily_f107 = float(encoded[39:44])
        except (ValueError, OverflowError) as error:
            raise UpdateError(f"IRI AP/F10.7 line {line_number} is invalid") from error
        if daily_f107 <= 0:
            raise UpdateError(f"IRI AP/F10.7 line {line_number} lacks daily F10.7")
        if date in dates:
            raise UpdateError(f"IRI AP/F10.7 contains duplicate date {date.isoformat()}")
        dates.add(date)
    return complete_years(dates)


def rust_integer(value: int) -> str:
    return f"{value:_}"


def render_manifest(
    snapshot_ms: int,
    metadata: dict[str, tuple[str, tuple[int, int]]],
) -> bytes:
    gfz_hash, gfz_years = metadata["GFZ Kp/ap/Ap/SN/F10.7"]
    ig_hash, ig_years = metadata["IRI IG/Rz"]
    ap_hash, ap_years = metadata["IRI AP/F10.7"]
    text = f'''// @generated by scripts/update-cache.py; do not edit manually.

pub(crate) const SNAPSHOT_AT_UTC_MS: i64 = {rust_integer(snapshot_ms)};

pub(crate) const GFZ_SHA256: &str =
    "{gfz_hash}";
pub(crate) const GFZ_START_YEAR: u16 = {gfz_years[0]};
pub(crate) const GFZ_END_YEAR: u16 = {gfz_years[1]};

pub(crate) const IRI_IG_RZ_SHA256: &str =
    "{ig_hash}";
pub(crate) const IRI_IG_RZ_START_YEAR: u16 = {ig_years[0]};
pub(crate) const IRI_IG_RZ_END_YEAR: u16 = {ig_years[1]};

pub(crate) const IRI_APF107_SHA256: &str =
    "{ap_hash}";
pub(crate) const IRI_APF107_START_YEAR: u16 = {ap_years[0]};
pub(crate) const IRI_APF107_END_YEAR: u16 = {ap_years[1]};
'''
    return text.encode()


def render_index_readme(
    snapshot_date: str,
    metadata: dict[str, tuple[str, tuple[int, int]]],
) -> bytes:
    gfz_hash, gfz_years = metadata["GFZ Kp/ap/Ap/SN/F10.7"]
    ig_hash, ig_years = metadata["IRI IG/Rz"]
    ap_hash, ap_years = metadata["IRI AP/F10.7"]
    text = f'''# Bundled index snapshot

These verbatim upstream files are the deterministic fallback for the rolling
indices required by the currently integrated models. Online synchronization
still takes precedence. `SyncPolicy::Offline` imports only this snapshot and
existing local CAS objects, without creating an HTTP request.

Snapshot: {snapshot_date}

| File | Upstream | Complete years | SHA-256 |
|---|---|---:|---|
| `Kp_ap_Ap_SN_F107_since_1932.txt` | GFZ Potsdam | {gfz_years[0]}-{gfz_years[1]} | `{gfz_hash}` |
| `ig_rz.dat` | IRI Working Group | {ig_years[0]}-{ig_years[1]} | `{ig_hash}` |
| `apf107.dat` | IRI Working Group | {ap_years[0]}-{ap_years[1]} | `{ap_hash}` |

The GFZ file declares CC BY 4.0 for its geomagnetic indices and CC BY-NC 4.0
for contained sunspot numbers. IRI files retain their upstream data format and
provenance; see the project documentation for source links and citations.

Kyoto Dst and AE archives are deliberately not bundled: a complete historical
snapshot would make the crate disproportionately large. Offline Dst/AE access
therefore requires those files to have already been synchronized into the
user's local CAS.
'''
    return text.encode()


def update_root_readme(content: bytes, metadata: dict[str, tuple[str, tuple[int, int]]]) -> bytes:
    text = content.decode()
    replacement = (
        "The packaged snapshot covers complete GFZ years through "
        f"{metadata['GFZ Kp/ap/Ap/SN/F10.7'][1][1]}, IRI IG/Rz through\n"
        f"{metadata['IRI IG/Rz'][1][1]}, and IRI AP/F10.7 through "
        f"{metadata['IRI AP/F10.7'][1][1]}."
    )
    pattern = re.compile(
        r"The packaged snapshot covers complete GFZ years through \d{4}, "
        r"IRI IG/Rz through\n\d{4}, and IRI AP/F10\.7 through \d{4}\."
    )
    updated, count = pattern.subn(replacement, text)
    if count != 1:
        raise UpdateError(f"expected one packaged-snapshot sentence in README.md, found {count}")
    return updated.encode()


def atomic_write(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as temporary:
            temporary.write(data)
            temporary.flush()
            os.fsync(temporary.fileno())
        os.replace(temporary_name, path)
    except BaseException:
        try:
            os.unlink(temporary_name)
        except FileNotFoundError:
            pass
        raise


def verify(include_models: bool) -> None:
    run(["cargo", "fmt", "--all", "--", "--check"])
    run(["cargo", "test", "-p", "ionoray-indices", "download::cache::tests"])
    run(
        [
            "cargo",
            "test",
            "-p",
            "ionoray-indices",
            "offline_policy_bootstraps_packaged_model_driver_cache",
        ]
    )
    package = subprocess.run(
        ["cargo", "package", "-p", "ionoray-indices", "--allow-dirty", "--no-verify", "--list"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    if package.returncode != 0:
        raise UpdateError(f"cargo package --list failed:\n{package.stderr}")
    for name in ("Kp_ap_Ap_SN_F107_since_1932.txt", "ig_rz.dat", "apf107.dat"):
        if f"cache/{name}" not in package.stdout:
            raise UpdateError(f"cargo package omitted cache/{name}")
    if include_models:
        with tempfile.TemporaryDirectory(prefix="geospace-rs-offline-target-") as target:
            env = os.environ.copy()
            env["IONORAY_OFFLINE"] = "1"
            env["CARGO_TARGET_DIR"] = target
            run(
                [
                    "cargo",
                    "check",
                    "-p",
                    "ionoray-msis",
                    "-p",
                    "ionoray-hwm",
                    "-p",
                    "ionoray-iri",
                ],
                env=env,
            )


def main() -> int:
    args = parse_args()
    sources = INDEX_SOURCES if args.indices_only else INDEX_SOURCES + MODEL_SOURCES
    staged: dict[Path, bytes] = {}
    metadata: dict[str, tuple[str, tuple[int, int]]] = {}
    transports: dict[str, str] = {}
    snapshot = dt.datetime.now(dt.timezone.utc)

    with tempfile.TemporaryDirectory(prefix="geospace-rs-cache-download-") as directory:
        temporary_dir = Path(directory)
        for index, source in enumerate(sources):
            print(f"Downloading {source.name} ...", flush=True)
            output = temporary_dir / str(index)
            transports[source.name] = download(source, output, args.transport, args.remote_host)
            data = output.read_bytes()
            sha256 = digest(data)
            if source.pinned_sha256 and sha256 != source.pinned_sha256:
                raise UpdateError(
                    f"{source.name} changed upstream: expected pinned SHA-256 "
                    f"{source.pinned_sha256}, got {sha256}. Update the model manifest, "
                    "patches, and reference tests deliberately; this script will not upgrade it."
                )
            staged[source.destination] = data

        index_data = {source.name: staged[source.destination] for source in INDEX_SOURCES}
        metadata["GFZ Kp/ap/Ap/SN/F10.7"] = (
            digest(index_data["GFZ Kp/ap/Ap/SN/F10.7"]),
            gfz_coverage(index_data["GFZ Kp/ap/Ap/SN/F10.7"]),
        )
        metadata["IRI IG/Rz"] = (
            digest(index_data["IRI IG/Rz"]),
            ig_rz_coverage(index_data["IRI IG/Rz"]),
        )
        metadata["IRI AP/F10.7"] = (
            digest(index_data["IRI AP/F10.7"]),
            apf107_coverage(index_data["IRI AP/F10.7"]),
        )

    staged[MANIFEST] = render_manifest(int(snapshot.timestamp() * 1000), metadata)
    staged[INDEX_README] = render_index_readme(snapshot.date().isoformat(), metadata)
    staged[ROOT_README] = update_root_readme(ROOT_README.read_bytes(), metadata)

    print("\nValidated cache batch:")
    for source in sources:
        data = staged[source.destination]
        old_hash = (
            digest(source.destination.read_bytes()) if source.destination.exists() else "missing"
        )
        coverage = metadata.get(source.name)
        suffix = f", complete years {coverage[1][0]}-{coverage[1][1]}" if coverage else ""
        change = "unchanged" if old_hash == digest(data) else f"{old_hash} -> {digest(data)}"
        print(f"  {source.name}: {change}{suffix} via {transports[source.name]}")

    if args.check:
        print("Check mode: no files written.")
        return 0

    originals = {path: path.read_bytes() if path.exists() else None for path in staged}
    try:
        for path, data in staged.items():
            atomic_write(path, data)
        if not args.no_verify:
            verify(not args.indices_only)
    except BaseException:
        print("Update failed; restoring the previous cache batch.", file=sys.stderr)
        for path, original in originals.items():
            if original is None:
                path.unlink(missing_ok=True)
            else:
                atomic_write(path, original)
        raise

    print("Cache batch updated successfully.")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except UpdateError as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1) from error

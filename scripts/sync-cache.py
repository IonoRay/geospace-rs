#!/usr/bin/env python3
"""Standalone GitHub cache sync; Python 3.10+, standard library only.

The remote source must exist and permit the requested use before synchronization.
Run: python3 sync-cache.py --dest /path/to/main-checkout --dry-run
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request

API = "https://api.github.com"
RAW = "https://raw.githubusercontent.com"
MANIFEST_PATH = "assets/cache-manifest.json"
RECEIPT_PATH = ".ionoray-cache/sync.json"
PROJECT = "IonoRay/geospace-rs"
MAX_FILE_BYTES = 100 * 1024 * 1024
ASSET_GROUPS = {
    "crates/models/hwm/cache/hwm14.tgz": "hwm",
    "crates/models/hwm/data/reference-profiles.txt": "hwm",
    "crates/models/msis/cache/nrlmsis2.1.tar.gz": "msis",
    "crates/models/iri/cache/iri2020.tar": "iri",
    "crates/models/igrf/data/igrf14coeffs.txt": "igrf",
    "crates/indices/cache/Kp_ap_Ap_SN_F107_since_1932.txt": "indices",
    "crates/indices/cache/ig_rz.dat": "indices",
    "crates/indices/cache/apf107.dat": "indices",
}
NOTICE_PATHS = {
    "LICENSE", "LICENSE-MIT", "LICENSE-APACHE", "THIRD_PARTY_NOTICES.md",
    "crates/models/iri/IRI-LICENSE.txt",
    "crates/models/msis/nrlmsis2.1_license.txt",
    "crates/indices/IRI-LICENSE.txt", "crates/indices/GFZ-NOTICE.txt",
}


class SyncError(Exception):
    """An actionable failure without exposing request credentials."""


class SameHostRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        old, new = urllib.parse.urlsplit(request.full_url), urllib.parse.urlsplit(newurl)
        if new.scheme != old.scheme or new.netloc != old.netloc:
            raise SyncError("refusing a redirect to another host or scheme")
        return super().redirect_request(request, fp, code, msg, headers, newurl)


def read_remote(url: str, limit: int, token: str = "", raw: bool = False) -> bytes:
    headers = {"User-Agent": "ionoray-cache-sync/1", "Accept": "application/vnd.github+json"}
    if raw:
        headers["Accept"] = "application/vnd.github.raw+json"
    if token:
        if not token.isascii() or any(ord(c) < 33 or ord(c) == 127 for c in token):
            raise SyncError("GitHub token contains invalid header characters")
        if urllib.parse.urlsplit(url).netloc != urllib.parse.urlsplit(API).netloc:
            raise SyncError("credentials may only be sent to the GitHub API")
        headers["Authorization"] = f"Bearer {token}"
    request = urllib.request.Request(url, headers=headers)
    opener = urllib.request.build_opener(SameHostRedirect())
    try:
        with opener.open(request, timeout=30) as response:
            data = response.read(limit + 1)
    except urllib.error.HTTPError as error:
        code = error.code
        error.close()
        raise SyncError(f"GitHub HTTP {code}; check ref, access and API rate limit") from None
    except (urllib.error.URLError, TimeoutError) as error:
        raise SyncError(f"GitHub request failed ({type(error).__name__}); retry the sync") from None
    if len(data) > limit:
        raise SyncError("remote response exceeds the declared size limit")
    return data


def decode_json(data: bytes, label: str) -> dict:
    try:
        value = json.loads(data)
    except (ValueError, UnicodeError):
        raise SyncError(f"{label} is not valid JSON") from None
    if not isinstance(value, dict):
        raise SyncError(f"{label} must be a JSON object")
    return value


def resolve_commit(repo: str, ref: str, token: str) -> str:
    if re.fullmatch(r"[0-9a-fA-F]{40}", ref):
        return ref.lower()
    url = f"{API}/repos/{repo}/commits/{urllib.parse.quote(ref, safe='')}"
    commit = decode_json(read_remote(url, 1024 * 1024, token), "commit response").get("sha")
    if not isinstance(commit, str) or not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise SyncError("GitHub did not return a full commit SHA")
    return commit


def fetch_file(repo: str, commit: str, path: str, limit: int, token: str) -> bytes:
    encoded = urllib.parse.quote(path, safe="/")
    if token:
        url = f"{API}/repos/{repo}/contents/{encoded}?ref={commit}"
        return read_remote(url, limit, token, raw=True)
    return read_remote(f"{RAW}/{repo}/{commit}/{encoded}", limit)


def validate_manifest(manifest: dict, project: str = PROJECT) -> list[dict]:
    if type(manifest.get("schema_version")) is not int or manifest["schema_version"] != 1:
        raise SyncError("unsupported manifest schema_version")
    if manifest.get("repository") != project:
        raise SyncError("manifest belongs to another project")
    entries = manifest.get("files")
    if not isinstance(entries, list) or not entries or len(entries) > 32:
        raise SyncError("manifest files must contain 1 to 32 entries")
    seen = set()
    for entry in entries:
        if not isinstance(entry, dict):
            raise SyncError("manifest file entry must be an object")
        path = entry.get("path")
        if not isinstance(path, str):
            raise SyncError("manifest path must be a string")
        pure = PurePosixPath(path)
        if pure.is_absolute() or ".." in pure.parts or pure.as_posix() != path:
            raise SyncError("manifest contains a non-canonical relative path")
        if path in seen or path not in ASSET_GROUPS and path not in NOTICE_PATHS:
            raise SyncError("manifest contains duplicate or unapproved paths")
        seen.add(path)
        group = ASSET_GROUPS.get(path, "notices")
        if entry.get("group") != group:
            raise SyncError(f"manifest group mismatch: {path}")
        digest = entry.get("sha256")
        if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise SyncError(f"invalid SHA-256: {path}")
        size = entry.get("size")
        if type(size) is not int or not 0 <= size <= MAX_FILE_BYTES:
            raise SyncError(f"invalid size: {path}")
    if seen != set(ASSET_GROUPS) | NOTICE_PATHS:
        raise SyncError("manifest must list the complete approved assets and notices")
    return entries


def safe_target(root: Path, relative: str) -> Path:
    target = root.joinpath(*PurePosixPath(relative).parts)
    cursor = root
    for part in PurePosixPath(relative).parts:
        cursor = cursor / part
        if cursor.is_symlink():
            raise SyncError(f"refusing a symlink in destination: {relative}")
    return target


def file_digest(path: Path) -> str | None:
    if not path.exists():
        return None
    if not path.is_file():
        raise SyncError(f"destination is not a regular file: {path.name}")
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_receipt(root: Path, repo: str) -> tuple[dict, str | None]:
    path = safe_target(root, RECEIPT_PATH)
    identity = file_digest(path)
    if identity is None:
        return {}, None
    receipt = decode_json(path.read_bytes(), "local receipt")
    files = receipt.get("files")
    if receipt.get("repository") != repo or not isinstance(files, dict):
        raise SyncError("local receipt belongs to another repository or is malformed")
    for path, digest in files.items():
        if path not in ASSET_GROUPS and path not in NOTICE_PATHS:
            raise SyncError("local receipt contains an unapproved path")
        if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise SyncError("local receipt contains an invalid digest")
    return files, identity


def sync(args) -> dict:
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repo):
        raise SyncError("--repo must be OWNER/REPOSITORY")
    requested = Path(args.dest).expanduser()
    if requested.is_symlink():
        raise SyncError("destination root is a symlink")
    root = requested.resolve()
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN", "")
    commit = resolve_commit(args.repo, args.ref, token)
    manifest_bytes = fetch_file(args.repo, commit, MANIFEST_PATH, 1024 * 1024, token)
    manifest = decode_json(manifest_bytes, "remote manifest")
    entries = validate_manifest(manifest)
    expected_path = Path(args.manifest) if args.manifest else safe_target(root, MANIFEST_PATH)
    if args.manifest or expected_path.exists():
        expected = decode_json(expected_path.read_bytes(), "expected manifest")
        validate_manifest(expected)
        if expected != manifest:
            raise SyncError("cache manifest differs from the main checkout; use a matching --ref")
    groups = set(args.only) if args.only else set(ASSET_GROUPS.values())
    selected = [e for e in entries if e["group"] == "notices" or e["group"] in groups]
    previous, receipt_identity = load_receipt(root, args.repo)
    changes, reused, identities = [], [], {}
    for entry in selected:
        path, expected = entry["path"], entry["sha256"]
        target = safe_target(root, path)
        actual = file_digest(target)
        identities[path] = actual
        if actual == expected and target.stat().st_size == entry["size"]:
            reused.append(path)
        elif actual is None or actual == previous.get(path):
            changes.append(entry)
        else:
            raise SyncError(f"local file is changed or unmanaged; preserved: {path}")
    print(f"source={args.repo} ref={args.ref} commit={commit}")
    for entry in changes:
        print(f"fetch {entry['path']} ({entry['size']} bytes)")
    print(f"reuse={len(reused)} fetch={len(changes)} dry_run={args.dry_run}")
    result = {"commit": commit, "reused": reused, "downloaded": [e["path"] for e in changes]}
    if args.dry_run:
        return result
    root.mkdir(parents=True, exist_ok=True)
    # Verify all transfers before replacing any destination file.
    with tempfile.TemporaryDirectory(prefix="ionoray-cache-sync-", dir=root) as staging:
        stage = Path(staging)
        for number, entry in enumerate(changes):
            data = fetch_file(args.repo, commit, entry["path"], entry["size"], token)
            if len(data) != entry["size"] or hashlib.sha256(data).hexdigest() != entry["sha256"]:
                raise SyncError(f"download size/SHA-256 mismatch: {entry['path']}")
            (stage / str(number)).write_bytes(data)
        # Recheck every selected file, including files reused without transfer.
        for entry in selected:
            if file_digest(safe_target(root, entry["path"])) != identities[entry["path"]]:
                raise SyncError(f"destination changed during download: {entry['path']}")
        if file_digest(safe_target(root, RECEIPT_PATH)) != receipt_identity:
            raise SyncError("local sync receipt changed during download")
        for number, entry in enumerate(changes):
            target = safe_target(root, entry["path"])
            target.parent.mkdir(parents=True, exist_ok=True)
            if file_digest(safe_target(root, entry["path"])) != identities[entry["path"]]:
                raise SyncError(f"destination changed before replace: {entry['path']}")
            os.replace(stage / str(number), target)
        recorded = {**previous, **{e["path"]: e["sha256"] for e in selected}}
        receipt = {"schema_version": 1, "repository": args.repo, "requested_ref": args.ref,
                   "commit": commit, "manifest_sha256": hashlib.sha256(manifest_bytes).hexdigest(),
                   "files": recorded}
        receipt_path = safe_target(root, RECEIPT_PATH)
        receipt_path.parent.mkdir(parents=True, exist_ok=True)
        if file_digest(safe_target(root, RECEIPT_PATH)) != receipt_identity:
            raise SyncError("local sync receipt changed before replace")
        receipt_temp = stage / "receipt.json"
        receipt_temp.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
        os.replace(receipt_temp, receipt_path)
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default="IonoRay/geospace-rs")
    parser.add_argument("--ref", default="cache-snapshots", help="branch, tag or full commit SHA")
    parser.add_argument("--dest", required=True, help="main checkout root or standalone cache root")
    parser.add_argument("--manifest", help="trusted local compatibility manifest; otherwise auto-detect")
    parser.add_argument("--only", action="append", choices=sorted(set(ASSET_GROUPS.values())))
    parser.add_argument("--dry-run", action="store_true", help="read remote manifest, no asset download or writes")
    args = parser.parse_args()
    try:
        sync(args)
    except (SyncError, OSError) as error:
        print(f"cache sync failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

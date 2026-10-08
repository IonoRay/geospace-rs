# main / cache-snapshots release workflow

The two local branches share one fresh history. `main` contains the implementation,
notices, scripts and `assets/cache-manifest.json`, without the eight raw snapshots.
`cache-snapshots` adds only those eight paths. Old asset-containing histories stay
on local backup references; never publish them with `--all`, `--mirror` or all tags.
Engineering checks do not grant upstream distribution rights.

## Build-time sources

All commands run from the repository root through `nix develop .#default --command`.
The default Rust assembly and Python bindings need no model snapshot. Python's
base exposes indices/Session and explicit `model_unavailable` failures for direct
uncompiled model calls. Prepared classes and Session model methods follow the
compiled features, as reported by `capabilities()`.

Fixed-model acquisition uses explicit source directories first, verified Cargo
OUT_DIR assets next, an explicit cache root or local fixed snapshot next, and
permitted build-time network acquisition last. Selected corrupt files fail hash
verification, including when a valid OUT_DIR already exists. Ordinary builds never
write downloaded assets into the source tree. Runtime model evaluation uses only
compiled model assets and explicit inputs, never a store/network/cache-root lookup.

| Model | Explicit source | Model offline switch |
|---|---|---|
| IGRF-14 | `IONORAY_IGRF14_COEFFICIENT_FILE` (file) | `IONORAY_IGRF14_OFFLINE` |
| HWM14 | `IONORAY_HWM14_SOURCE_DIR` (official HWM14 directory, including Check/gfortran.txt) | `IONORAY_HWM14_OFFLINE` |
| IRI-2020 | `IONORAY_IRI2020_SOURCE_DIR` (IRI-zip directory or its parent) | `IONORAY_IRI2020_OFFLINE` |
| NRLMSIS 2.1 | `IONORAY_NRLMSIS21_SOURCE_DIR` (flat official directory) | `IONORAY_NRLMSIS21_OFFLINE` |

`IONORAY_CACHE_ROOT=/absolute/root` uses the exact `crates/.../cache` and `data`
layout from the shared manifest. `IONORAY_OFFLINE=1` prohibits all model acquisition
HTTP. Presence of any offline switch selects offline mode, matching the original
switch convention. Cargo `--offline` is a separate dependency-resolution setting.

Indices embed optional fixed snapshots at build time. Refresh checks upstream;
Ensure reuses complete local coverage; Offline performs no HTTP and reports gaps
when local CAS/optional snapshots are insufficient. The fixed coverage is GFZ
1932–2025, IRI IG/Rz 1958–2026 and IRI AP/F10.7 1958–2024. Dst/AE need previously
accepted local data. Raw negative/NULL values, hashes and provenance are retained.

```bash
# Default base, with no snapshots:
nix develop .#default --command env IONORAY_OFFLINE=1 \
  cargo test --locked --offline -p ionoray-core -p ionoray-python -p ionoray-indices

# Both branches use this same full-model implementation and input:
nix develop .#default --command env IONORAY_CACHE_ROOT=/absolute/approved-cache-root IONORAY_OFFLINE=1 \
  cargo run --locked --offline -p ionoray-geospace --features standard --example direct_models

# cache-snapshots carries the same verified files, so an external root is optional:
nix develop .#default --command env IONORAY_OFFLINE=1 \
  cargo test --locked --offline --workspace --exclude ionoray-python --all-features
nix develop .#default --command env IONORAY_OFFLINE=1 \
  cargo test --locked --offline -p ionoray-python --features standard
```

The direct example supplies every scientific driver and opens no data home.
Check magnetic/wind/density values and fixed asset hashes; source-mode provenance
may differ across a supplied directory, local snapshot and Cargo OUT_DIR reuse.

## Standalone GitHub synchronization

`scripts/sync-cache.py` needs Python 3.10+ standard library only. It resolves a
branch to one full commit, checks the complete path/size/SHA-256 manifest and
compatibility with a main checkout, stages all transfers before replacement,
protects changed/unmanaged files and writes `.ionoray-cache/sync.json`. A failed
multi-file installation can leave a verified subset; rerunning completes it.
It is not a filesystem transaction or a signature verifier. It neither extracts
archives nor invokes Git/Cargo. Tokens are sent only to the GitHub API; redirects
to other hosts/schemes are rejected. Notices accompany every partial selection.

A GitHub source must exist and have appropriate sharing rights first. The local
cache branch is not advertised as a published/authorized remote source. When
that condition is satisfied, replace the parameters below with the actual source:

```bash
curl --fail --location --connect-timeout 15 --max-time 60 \
  https://raw.githubusercontent.com/OWNER/APPROVED-REPO/FULL_COMMIT_SHA/scripts/sync-cache.py \
  --output /path/to/sync-cache.py
python3 /path/to/sync-cache.py --repo OWNER/APPROVED-REPO --ref FULL_COMMIT_SHA \
  --dest /absolute/cache-root --only igrf --only indices
python3 /path/to/sync-cache.py --repo OWNER/APPROVED-REPO --ref FULL_COMMIT_SHA \
  --dest . --dry-run
```

Default `--repo` is IonoRay/geospace-rs and default `--ref` is cache-snapshots.
Authorized private mirrors can supply `GH_TOKEN`/`GITHUB_TOKEN`; no token is
printed or written into receipts. Syncing into main creates ignored snapshots,
not tracked source changes. Use a standalone root to keep the checkout empty.

## Checks and artifacts

```bash
nix develop .#default --command cargo fmt --all -- --check
nix develop .#default --command env IONORAY_OFFLINE=1 \
  cargo clippy --locked --offline --workspace --all-targets --all-features --no-deps -- -D warnings
nix develop .#default --command env IONORAY_OFFLINE=1 RUSTDOCFLAGS=-Dwarnings \
  cargo doc --locked --offline --workspace --all-features --no-deps
nix develop .#default --command python3 scripts/test_sync_cache.py -v
nix develop .#default --command python3 scripts/test_update_cache.py -v
nix develop .#default --command python3 scripts/test_cache_integration.py --cache-root /absolute/approved-cache-root
nix develop .#default --command python3 scripts/test_build_sources.py --cache-root /absolute/approved-cache-root -v
nix develop .#default --command python3 scripts/check-release.py --packages
```

On main, full-model Clippy/tests/doc require the explicit source/cache root above;
otherwise the intentional clean offline source error is the expected outcome.
`test_build_sources.py` uses the build-script executables from a preceding Cargo
build and fresh OUT_DIRs. It covers missing/corrupt sources, valid explicit cache,
verified reuse and indices Some/None switching. A separate clean Cargo target
verifies that compiled-model results do not depend on old build artifacts.

All crates include their original license texts and scoped third-party notices.
Source packages/sdist exclude the eight snapshots even when locally installed.
HWM/MSIS registry publication is disabled until the obligations below are resolved.
To inspect an actual original-code crate, `cargo package --locked --offline
--allow-dirty --no-verify -p ionoray-core` produces its `.crate`; dependent registry
packages must be published in dependency order and require separate registry evidence.

Python model features are `igrf`, `iri`, `hwm`, `msis`, and `standard`.
`extension-module` is for wheel construction, not Rust test executables.
Wheel scope is declared by `LicenseRef-IonoRay-Distribution` and the included
license texts, not by an invented uniform upstream SPDX grant.

```bash
nix develop .#default --command maturin build --locked --offline --no-default-features \
  --features extension-module --auditwheel repair --out /absolute/base-wheels
nix develop .#default --command env IONORAY_OFFLINE=1 IONORAY_CACHE_ROOT=/absolute/approved-cache-root \
  maturin build --locked --offline --no-default-features --features extension-module,standard \
  --auditwheel repair --out /absolute/research-wheels
nix develop .#default --command maturin sdist --out /absolute/source-distributions
nix develop .#default --command python3 scripts/check-python-artifacts.py --wheel /absolute/base.whl
nix develop .#default --command python3 scripts/check-python-artifacts.py --wheel /absolute/standard.whl \
  --models igrf,iri,hwm,msis --full-suite
nix develop .#default --command python3 scripts/check-python-artifacts.py --sdist /absolute/source.tar.gz
```

The full Python suite compares against the existing Rust `driver_scenarios`
example. Supply the same documented explicit model source directories to both
builds and build the comparator before checking the wheel. Acquisition provenance
is part of the complete equality check: verified cache reuse can legitimately
record cargo-out-dir-cache in one build and bundled-cache in another. Matching
explicit SOURCE_DIR inputs make that comparison controlled without dropping any
source fields. The artifact checker
extracts into a temporary site and installs no package; child interpreters use
that same wheel. macOS checks actual dynamic dependencies for Nix-store paths.
On macOS the binding drops unused dylibs; full models still require actual Fortran
runtime repair and licensing review. No Linux/Windows/MSRV claim follows from a
macOS arm64/CPython 3.14 check. Registry upload, remote default-branch replacement,
remote cache synchronization and observational/scientific acceptance are separate.

Process-level OnceLock guards retain randomly named verified asset directories.
Local TempDir Drop/close cleanup is tested. Rust statics are not dropped at exit,
so process-global cleanup is not promised. NRLMSIS keeps its 128-byte path limit.
Existing VS Code examples/CodeLens reuse these business functions; use core
`position::serde_tests::nested_query_point_cannot_bypass_position_validation`
and model `backend::asset_tests` to inspect values. Terminal checks and an actual
VS Code breakpoint hit are distinct; the editor/user confirmation remains pending.
Full-model VS Code build tasks prompt for an absolute approved cache root. Leave
it empty on cache-snapshots, which supplies local files, or when providing explicit
source directories; enter the external root on main. The prompt applies to the
build task itself, since launch-time debugger environment alone cannot configure
Cargo acquisition. Python debugging additionally needs the existing documented
venv/debugpy setup. No editor breakpoint hit is claimed by terminal builds.

## Maintaining the branches

Make implementation changes on main. Then, in the cache-snapshots checkout,
merge main in the ordinary direction and rerun `check-release.py`. Common files
must be identical; only the eight manifest assets may differ. Do not merge
cache-snapshots or asset commits back into main.

`scripts/update-cache.py --dest /absolute/preparation-root` prepares official
upstream assets and metadata outside the source checkout. Its default is a fresh
temporary root. Coverage/parser/hash checks retain scientific identities; it
refuses changed pinned releases. It outputs metadata for review and does not
rewrite main. Apply approved common hash/coverage/manifests on main first; merge
main into cache-snapshots, then commit the matching snapshot bytes there.
`--indices-only` is a partial preparation; combine it with all approved fixed
assets before creating the complete shared GitHub manifest. Missing fixed assets
with an existing output manifest cause an explicit stale-manifest error. All
prepared files are validated before writing, and a failed local verification
restores their previous bytes. Independent reference
scripts accept `--archive` and replay official inputs without Rust-generated answers.

## Public release blockers

HWM14's pinned software/derived adapter authorization is unresolved. MSIS's exact
agreement restricts use and requires review of section 4(b) modification/delivery
and 4(c) notices; no delivery to NRL is claimed. Keeping assets local or using a
private remote is not an exemption. Removing archives from main also does not
settle the adapter/reference-output obligations.

GFZ includes SN with CC BY-NC 4.0 inside the original file; the original header
is retained in GFZ-NOTICE.txt. CC terms may be supplied by their provider license
URLs; IRI/MSIS original agreements accompany the applicable packages.

The full macOS repaired wheel additionally copies GNU runtime libraries and
Apple libiconv/libcharset. GNU runtime/library texts are retained, but review the
actual library versions, corresponding source and upstream/Nix modifications
before binary redistribution. The Apple libcharset source header identifies
APSL-1.0 while package metadata lists BSD terms; this discrepancy remains open.
Do not infer binary permission from Nix/Cargo license metadata alone.

The local dependency inventory retains Cargo declarations and 619 original
license/notice texts from the registry cache. It is an all-features inventory
superset, not a per-binary rights clearance; 28 packages lack separately packaged
license texts. A public binary also needs its actual Rust dependency copyright
and notice obligations checked and included. Source-package notices alone do not
complete that binary review.

No remote push/rewrite, public asset branch, registry upload, NRL contact or binary
publication is part of this implementation. Those actions require concrete rights
and remote/platform checks after this local delivery. See the dated
[implementation evidence](main-cache-validation.md) for observed results and limits.

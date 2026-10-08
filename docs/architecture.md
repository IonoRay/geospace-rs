# Architecture baseline

## Dependency layers

```text
ionoray-python -- ionoray-geospace (indices + explicit optional model features)
    `-- ionoray-observability
ionoray-geospace
    |-- ionoray-indices -- ionoray-store -- ionoray-core
    |-- model crates ---------------------- ionoray-core
    `-- ionoray-observability (CLI only)
```

- `ionoray-core` owns stable time, position, unit, identifier, and provenance types.
- `ionoray-store` owns Turso databases, the local layout, schema rebuilds, and CAS/download logic.
- `ionoray-indices` is an independent Kp/Ap/F10.7/Dst/AE data product.
- Each model family is an independent synchronous executor with explicit inputs.
- `ionoray-geospace` is the only assembly layer and also owns the `geospace` binary.
- `ionoray-observability` owns process-level tracing configuration. Model and
  data libraries emit spans but do not install subscribers; their examples use
  `ionoray-observability` only as a development dependency.
- `ionoray-python` owns PyO3 argument/result conversion, thread-confined
  Session runtime/store lifecycle, and independent Prepared wrappers. It calls
  the existing model and preparation APIs
  and adds no Python dependencies to the existing Rust library crates.

Runtime model evaluation never queries indices, Turso, the network, or the local
data home. The data product never depends on model crates.

## Upstream dependency decisions

- `hifitime` is the canonical epoch type because it supports UTC, TAI, GPS, Julian
  dates, and leap seconds. It is pinned exactly because its upstream versioning
  policy permits minor-version breaking changes.
- `uom` represents quantities crossing public APIs. Model ABI adapters convert to
  documented scalar units only at the boundary.
- `nalgebra` belongs in model crates that perform vector and matrix work; it is not
  a core dependency.
- `geo` and `geo-types` target 2D geometry and spatial algorithms. They will be
  added only when coverage polygons, tracks, or spatial predicates are implemented.
- `proj` is deferred to an optional CRS adapter. It brings a native PROJ dependency
  and is not required for geodetic point inputs or model-local ECEF/ENU transforms.
- `turso` is pinned and isolated in `ionoray-store`. No public domain type exposes
  Turso connections or SQL rows.
- `reqwest` uses rustls and is isolated in `ionoray-store`; model crates remain
  network-free.

The Nix development shell uses `oxalica/rust-overlay`'s `stable.latest.default`
profile with Clippy, rustfmt, rust-analyzer, and rust-src. `flake.lock` pins the
actual compiler; `nix flake update` is the explicit update operation.

The checked-in VS Code rust-analyzer configuration uses
`rust-analyzer.cargo.features: "all"`, matching the documented
[cargo.features setting](https://rust-analyzer.github.io/book/configuration.html#rust-analyzercargofeatures),
with `--locked --offline` and `IONORAY_OFFLINE=1` for Cargo and runnables. Those
are build-time constraints; the launch arguments and runtime policy still decide
whether a data operation can access the network.

## Runtime data home

Resolution order is explicit CLI/API path, `IONORAY_HOME`, then
`~/.ionoray/geospace-rs`. The explicit and environment selections may name an
absolute path or a `~/...` path, which `StoreHome` expands before validation.
Relative paths are rejected.

```text
~/.ionoray/geospace-rs/
|-- objects/
|   `-- <scope>/
|       |-- catalog.db
|       |-- sha256/<prefix>/<digest>
|       `-- <canonical-host>/<remote-path>/<accepted-UTC>__<filename> -> CAS
|-- indices/
|   |-- gfz/kp-ap-f107/<year>.db
|   |-- wdc-kyoto/{dst,ae}/<year>.db
|   `-- iri/{ig-rz,apf107}/<year>.db
`-- tmp/<scope>/
```

An old `~/.ionoray/indices` or `~/.ionoray/objects` is not silently treated as
the new default. When neither an explicit root nor `IONORAY_HOME` selects one,
`StoreHome::discover` returns a selection error. The caller must choose the old
or new directory explicitly; this boundary performs no migration, removal,
copying, or duplicate download.

`IndexStore::init` and `geospace data init` initialize the indices root.
`Store::open_root` creates no global catalog; scoped catalogs open lazily.
Every dependent operation calls the same initialization path, so init is never a
required separate step. Year databases are created lazily.

There is no migration chain during the current development phase. Each database
contains one exact schema identity. An incompatible object catalog or yearly
database is rebuilt independently. Rebuilding a year database preserves the
object catalog and can be recovered with `reindex_year`. Rebuilding the object
catalog intentionally discards its mappings; CAS bytes remain immutable and are
reattached by the next forced or normal upstream synchronization.

Each `objects/<scope>/catalog.db` maps its own digests to provider, dataset, year, month,
publication edition, source URL, final URL, original filename, fetch attempts,
remote timestamps, local size, mtime, and maintenance runs. Parsed values live
only in the source-specific yearly databases.

## Download and CAS state machine

```text
querying
   |-- HEAD only, unchanged -----------> available
   |-- HEAD only, changed -------------> metadata_changed
   |-- HEAD unsupported ---------------> needs_content_check
   |-- HTTP 404/410 -------------------> missing
   |-- matching HEAD metadata --------> metadata_unchanged
   |-- HTTP 304 ----------------------> not_modified
   |-- HTTP 200 + new SHA-256 --------> downloaded
   |-- HTTP 200 + existing SHA-256 ---> unchanged
   `-- HTTP, integrity, or I/O error --> failed
```

Every request creates a `fetch_attempt`, including failures and no-change
checks. Default checks first compare the local regular-file type, byte size, and
mtime. Suspicious local metadata triggers a full SHA-256 fallback. The remote is
then checked with URL, ETag/Last-Modified, and Content-Length through conditional
HEAD. Missing metadata or unsupported HEAD falls back to GET. `ForceContent`
fully hashes the local CAS object and unconditionally downloads the remote body.

Bodies stream to a unique `.part` file under `tmp/`, are flushed and synchronized,
then committed to `objects/<scope>/sha256/<prefix>/<digest>` with a no-overwrite hard link.
Existing objects are verified before reuse, and newly committed objects are marked
read-only. Artifact, source timestamp, HTTP metadata, exact byte count, local mtime,
query time, download time, and terminal status are retained in Turso. Each
successful artifact also creates an `artifact_origin` mapping back to its actual
upstream URL and filename. `Store::artifact_origins` resolves those mappings by
digest, including redirects and raw `Content-Disposition` metadata.

Every terminal fetch attempt and parsed release transaction is followed by
`PRAGMA wal_checkpoint(TRUNCATE)`. The checkpoint result and duration are traced;
a busy or malformed checkpoint fails the operation instead of reporting durable
success while committed frames remain only in the WAL.

## Index maintenance and version resolution

```text
init -> query -> download -> validate -> import -> verify -> read
```

- `query_year` performs metadata-only upstream checks.
- `download_year` maintains raw CAS files without importing values.
- `sync_year` checks upstream, downloads changes, validates, and imports.
- GFZ and IRI rolling-source failures fall back to a SHA-256-pinned snapshot
  optionally embedded at build time from `IONORAY_CACHE_ROOT` or local snapshot paths; offline policy starts from accepted local CAS
  bodies and uses the packaged snapshot as a fallback without HTTP requests.
- `ensure_year` never checks upstream when complete local coverage exists.
- `sync_range` is the model-driver preparation boundary: it requests only the
  fields and UTC interval actually needed, then returns explicit residual gaps.
- `reindex_year` rebuilds yearly databases from CAS without networking.
- `repair_year` redownloads complete bodies and repairs corrupt CAS files.

Annual imports use explicit active and retained-history pointers per monthly or
rolling partition. Semantic fingerprints include values, missingness, quality,
and derivation metadata. Append does not create a new historical snapshot;
revision retains the prior current state. Sample provenance records the selected
source when a lower-maturity candidate fills a genuine current gap. Read queries
join active pointers rather than sorting every historical release.

`IndexStore::sync_range` audits the required fields over `[start, end)` and
reports local `coverage_status`, upstream `source_check_status`, and a strict
combined `status`, with remaining gaps and all source attempts. `Offline`,
`Ensure`, and `Refresh` keep HTTP policy outside model crates. The range CLI and
assembly-layer model preparation use this boundary. See the
[data maintenance guide](data-maintenance.md) for commands and validation boundaries.

Build and runtime offline controls remain separate: Cargo `--offline` controls
dependency resolution, while `IONORAY_OFFLINE=1` requires verified existing build assets.
`IONORAY_CACHE_ROOT` supplies a build-time asset root with the shared manifest
layout; IGRF also accepts `IONORAY_IGRF14_COEFFICIENT_FILE`. No runtime model
loads from this root or the network. Runtime HTTP access follows `DataPolicy`, range `SyncMode`, or annual
`SyncPolicy`; the latter supports `AlwaysCheck`, `ForceDownload`, and `Offline`.

## Tracing contract

CLI and example traces have independent console and file sinks. The console is
enabled by default, uses compact ANSI-colored text when stderr is a TTY, and
switches to NDJSON without ANSI escapes when redirected. The optional file sink
always appends NDJSON without ANSI through a guarded, non-lossy background
writer. Stable results remain JSON on stdout. Executables that explicitly initialize `ionoray-observability` expose the
following trace data. Library callers and the offline learning examples may
omit initialization; diagnostics are not required for scientific computation:

- `timestamp` is RFC 3339 in the computer's local UTC offset, such as
  `2026-07-15T11:27:29.624748+08:00`.
- `threadId`, `threadName`, `target`, `filename`, and `line_number` identify the
  execution context and jumpable source location.
- The root `ionoray.run` span carries `operation`, UUIDv7 `run_id`, and `pid`;
  nested records include the current span and complete span stack.
- Span close records carry `time.busy` and `time.idle`. Domain completion events
  additionally retain explicit `elapsed_ms` where the elapsed value is part of
  a report or durability record.

The default level is `info`. Routine per-item work, scientific inputs/results,
cache decisions, and HTTP outcomes use `debug`; phase and batch summaries use
`info`; recoverable corruption, rejected input, and returned operation errors
use `warn`; the CLI emits `error` when the command cannot complete. `RUST_LOG`
selects focused detail without changing code, for example
`RUST_LOG=ionoray_store=debug,ionoray_indices=info,ionoray_observability=info`.
`IONORAY_LOG_CONSOLE=on|off` controls the console sink and
`IONORAY_LOG_FILE=off|<path>` controls the file sink. The console defaults to on
and the file defaults to off. `IONORAY_LOG_FORMAT=auto|pretty|json` controls
console serialization, while `IONORAY_LOG_COLOR=auto|always|never` controls ANSI
color for console pretty output. `RUST_LOG` applies to both sinks.

## Geophysical index sources

- [GFZ](https://kp.gfz.de/fileadmin/files_for_gfz_cms/Kp_ap_Ap_SN_F107_since_1932.txt)
  publishes Kp, ap, Ap, and F10.7 in one rolling cumulative file. Each check may
  produce a new immutable CAS artifact.
- [WDC Kyoto](https://wdc.kugi.kyoto-u.ac.jp/dstdir/) Dst monthly sources are
  tried as final, provisional, then realtime.
- [WDC Kyoto](https://wdc.kugi.kyoto-u.ac.jp/aedir/) AE monthly sources are
  tried as provisional, then realtime for the currently supported 2020
  reference path.
- [IRI](https://irimodel.org/indices/) publishes rolling `ig_rz.dat` and
  `apf107.dat` files. The index product derives daily interpolated Rz12/IG12 and
  adjusted daily/81-day/365-day F10.7 while preserving source provenance.

`ionoray-indices` validates requested calendar coverage before returning success.
The 2020 reference check covers 366 GFZ daily rows, twelve final Dst monthly
files, twelve provisional AE monthly files, and 366 daily rows from each IRI
source.

## Implementation discipline

- Production Rust files stay below 500 lines; 300 lines is the normal split point.
- `lib.rs` files declare modules and re-export public API only.
- Unimplemented scientific models report an explicit error and never return dummy values.
- `prepare_*(request, policy).await` may access data; `Prepared*::evaluate(&self)` is synchronous and repeatable.

Application-facing IRI, HWM, and NRLMSIS requests contain a shared `QueryPoint`
(epoch plus WGS84 position) and optional environmental overrides. The assembly
layer resolves every missing driver from `ionoray-indices`, records the exact
selected samples, and produces a Prepared value for synchronous offline evaluation. IRI resolves
Rz12, IG12, and adjusted daily/81-day F10.7. HWM defaults to
disturbed winds using current three-hour ap. NRLMSIS defaults to F10.7a,
previous-day F10.7, daily Ap, and the complete official storm-time Ap history.
Explicit override fields take precedence and are not queried from the store.

`Prepared*` owns private input/evidence. `with_overrides` / `with_activity`
copy the baseline and clear evidence only for explicitly replaced drivers;
`evaluate` does not query data and remains usable after the store is dropped.
`evaluate_*(request, policy)` combines preparation and evaluation. IGRF stays
direct. Python Session owns its runtime/store, while CLI JSONL lazily opens one
store only when an automatic record needs it, retaining even an open failure.
There is no PointRequest/PointReport compatibility layer. Current usage and
examples are in [the Rust guide](rust-models.md),
[Python guide](python-models.md), and [CLI guide](cli-models.md).

## Model implementation stages

IGRF-14 is the first native model. `ionoray-igrf` embeds the verbatim official
coefficient table, verifies its SHA-256 at construction, selects degree 10 or 13
by epoch, and applies the 2025-2030 secular-variation model. Evaluation uses WGS84
geodetic input and returns east/north/up SI quantities plus declination,
inclination, total intensity, and coefficient provenance.

The native synthesis is checked against the official `IGRF14SYN` Fortran program
at the start and end of the published interval, across both harmonic degrees,
at altitude, and at both poles. Model spans record PID, input coordinates,
decimal year, coefficient resolution mode, harmonic degree, result components,
coefficient digest, and elapsed time. Timestamp and thread identity are added by
the process subscriber.

NRLMSIS 2.1 uses a build-time external-source backend because its upstream
license is not the workspace's MIT/Apache license. Cargo validates an explicit
`IONORAY_NRLMSIS21_SOURCE_DIR`, otherwise it tries the fixed official archive and
falls back to the verbatim SHA-256-pinned archive packaged with the model crate.
Every compiled source, parameter, and license file must match the official
manifest. Cargo then compiles the Fortran library locally and embeds the verified
parameter and license artifacts. Runtime evaluation is serialized around
upstream global state and remains fully offline. `IONORAY_OFFLINE=1` (or the
model-specific offline variable) selects the packaged archive without networking.

HWM14 follows the same external-source packaging boundary for the official NRL
supplemental release. Cargo validates `IONORAY_HWM14_SOURCE_DIR` or tries the
fixed archive before falling back to its packaged verified copy, then verifies
`hwm14.f90` and all three required coefficient files,
then compiles a generated explicit-path C ABI adapter. The adapter replaces only
the upstream `HWMPATH` file-discovery routine; it does not change scientific
calculations. At runtime the embedded coefficient set is hash-verified and
materialized atomically under the platform temporary directory. Initialization
is explicit and idempotent, evaluation calls it automatically, and all Fortran
access is serialized because the upstream implementation keeps global saved
state. The model remains independent of indices, Turso, the network, process
environment mutation, and `IONORAY_HOME`.

IRI-2020 pins the official 25 September 2025 source snapshot and follows the
same build-time source boundary, with the verified official archive packaged as
a deterministic fallback. Cargo verifies the release plus every
compiled source, numerical-map coefficient, Shubin-COSMIC coefficient, and IGRF
file. A generated adapter replaces relative asset lookup with an explicit
content-addressed directory and skips the otherwise unconditional AP/F10.7 file
lookup when all supported indices are caller supplied. The point API uses the
fixed climatology switches (including IRI-cor2 topside), absolute ion densities, and explicit Rz12,
IG12, daily F10.7, and 81-day F10.7 inputs. Extensions requiring magnetic-index
time series remain disabled rather than reading mutable files inside the model
crate. The internal NRLMSIS00 magnetic response is explicitly disabled with
`SWMI(9)=0`; the adapter removes its implicit `APFMSIS` lookup and initializes
the Ap array, date index and unused daily-Ap output sentinel. This follows the
upstream no-Ap-response branch, not an Ap=0 scenario. Independent replay uses
unmodified Fortran with synthetic unavailable-Ap sentinels; see the IRI README.
Runtime evaluation is serialized around upstream common blocks and is
independent of indices, Turso, the network, the process current directory, and
`IONORAY_HOME`.

# ionoray-msis

Independent Rust API for NRLMSIS neutral-atmosphere models.

## NRLMSIS 2.1 backend

The default `nrlmsis21` feature acquires the official NRL release at Cargo build
time, verifies the archive and every consumed file by SHA-256, generates a small
`ISO_C_BINDING` shim, and compiles a local static Fortran library in `OUT_DIR`.
The verbatim pinned archive is included in Git and the packaged crate sources as the
deterministic build fallback. Its restricted NRL license remains in the archive
and is also available as [nrlmsis2.1_license.txt](nrlmsis2.1_license.txt).

```bash
nix develop .#default --command cargo build -p ionoray-msis
```

For an already downloaded and extracted official archive, provide the directory
whose flat layout matches the NRL archive exactly:

```bash
IONORAY_NRLMSIS21_SOURCE_DIR=/path/to/nrlmsis2.1 \
  nix develop .#default --command cargo build -p ionoray-msis
```

To forbid network fallback:

```bash
IONORAY_OFFLINE=1 \
  nix develop .#default --command cargo build -p ionoray-msis
```

Resolution order is:

1. `IONORAY_NRLMSIS21_SOURCE_DIR`;
2. a complete verified source already present in Cargo `OUT_DIR`;
3. when online, the fixed [official NRL archive](https://map.nrl.navy.mil/map/pub/nrl/NRLMSIS/NRLMSIS2.1/nrlmsis2.1.tar.gz);
4. the packaged verified archive when offline or remote acquisition fails.

`IONORAY_NRLMSIS21_OFFLINE=1` remains available when only this backend should
be forced offline; `IONORAY_OFFLINE=1` applies to every integrated model build.

The parameter asset is embedded into the resulting Rust artifact and
hash-verified when materialized for the local Fortran initializer. Runtime model
evaluation performs no network, Turso, or `IONORAY_HOME` access.

`ionoray-msis` is intentionally the explicit, offline model boundary. Normal
applications should use `ionoray_geospace::Geospace::evaluate_msis` with an
`MsisRequest::new(query)`: the assembly layer resolves real GFZ F10.7a,
previous-day F10.7, daily Ap, and all three-hour ap bins required by the
official storm-time interface. `MsisDriverOverrides` can replace any field
individually. The `nrlmsis21_explicit_input` example remains the low-level
reference-reproduction path.

## Scientific inputs and numerical references

The solar inputs are the observed F10.7 flux at the actual Sun-Earth distance,
not the 1 AU-adjusted flux used by IRI: a centered 81-day mean and the previous
UTC day's value, both in sfu. `Daily` uses only daily Ap. `StormTime` requires
all seven official components: daily Ap, current/3/6/9-hour ap, and the means
over 12–33 and 36–57 hours before the query. A daily value is never expanded
into a fabricated history.

[`data/reference.json`](data/reference.json) contains a Daily point tied to the
official double-precision output table and a separate StormTime point evaluated
through the unmodified official source with `switch_legacy(9) = -1`. Replay both
without the Rust adapter:

```bash
nix develop .#default --command python3 scripts/verify_nrlmsis21_reference.py
nix develop .#default --command env IONORAY_OFFLINE=1 \
  cargo test --locked --offline -p ionoray-msis --lib
```

Each Rust case compares temperature, exospheric temperature, mass density, and
all nine species number densities in the SI units returned by `MSISCALC`.
Upstream missing-density sentinels become `None`, never zero. The reference
inputs, source/output hashes, output order, `5e-12` double-precision tolerance,
and scientific limits of the two-point check are recorded in
[`data/README.md`](data/README.md).

## License boundary

Original IonoRay code uses the workspace MIT/Apache-2.0 choice. Upstream
software and derived material retain the exact NRL agreement. See
[third-party notices](../../../THIRD_PARTY_NOTICES.md#nrlmsis-21--restricted-upstream-agreement),
including the section 4(b) modification/derivative-work obligations.

The official NRLMSIS 2.1 software is restricted to research, academic, and
non-profit purposes and prohibits commercial use without written permission.
Enabling the backend means accepting the exact upstream agreement embedded into
the resulting artifact and available through `nrlmsis21_license_bytes()`.

This software incorporates the MSIS empirical atmospheric model software
designed and provided by NRL. Use is governed by the Open Source Academic
Research License Agreement contained in `nrlmsis2.1_license.txt`.

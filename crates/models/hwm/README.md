# ionoray-hwm

Independent Rust API for the NRL Horizontal Wind Model.

## HWM14 backend

The default `hwm14` feature acquires the official HWM14.123114 supplemental
release at Cargo build time, verifies the archive and every consumed source/data
file by SHA-256, generates a small explicit-path `ISO_C_BINDING` adapter, and
compiles a local static Fortran library in `OUT_DIR`. The verbatim pinned archive
is included in Git and the packaged crate sources as the deterministic build fallback.

```bash
nix develop .#default --command cargo build -p ionoray-hwm
```

For an already downloaded and extracted official release, provide its flat
`HWM14/` directory (the directory containing `hwm14.f90`):

```bash
IONORAY_HWM14_SOURCE_DIR=/path/to/HWM14 \
  nix develop .#default --command cargo build -p ionoray-hwm
```

To forbid network fallback:

```bash
IONORAY_OFFLINE=1 \
  nix develop .#default --command cargo build -p ionoray-hwm
```

Resolution order is:

1. `IONORAY_HWM14_SOURCE_DIR`;
2. a complete verified source already present in Cargo `OUT_DIR`;
3. when online, the fixed [official NRL supplemental archive](https://map.nrl.navy.mil/map/pub/nrl/HWM/HWM14/);
4. the packaged verified archive when offline or remote acquisition fails.

`IONORAY_HWM14_OFFLINE=1` remains available when only this backend should be
forced offline; `IONORAY_OFFLINE=1` applies to every integrated model build.

## Data and initialization

HWM14 requires `hwm123114.bin`, `dwm07b104i.dat`, and `gd2qd.dat`. The verified
files are embedded in the Rust artifact, then materialized atomically into a
content-addressed directory under the platform temporary directory. The
generated adapter passes that directory explicitly to the Fortran backend; it
does not mutate the process-wide `HWMPATH` environment variable.

`Hwm::initialize()` validates/materializes the assets and initializes the
official backend once. Calling it is optional because `Hwm::evaluate()` invokes
the same idempotent path. Evaluation is serialized because the official source
uses module globals and saved working arrays. Runtime evaluation performs no
network, Turso, or `IONORAY_HOME` access.

The Rust API makes HWM14's activity modes explicit:

- `HwmGeomagneticActivity::Quiet` returns quiet-time climatological winds;
- `HwmGeomagneticActivity::Disturbed { current_ap }` adds DWM07 disturbance
  winds using the caller-supplied current three-hour ap index.

HWM14 ignores F10.7 and computes local solar time internally, so neither is
exposed as a misleading Rust input.

`ionoray-hwm` is intentionally the explicit, offline model boundary. Normal
applications should use `ionoray_geospace::Geospace::evaluate_hwm` with an
`HwmRequest::new(query)`: the assembly layer synchronizes GFZ data only when
needed, resolves the real three-hour ap sample, and returns that sample beside
the model result. Set `HwmRequest::geomagnetic_activity` only when deliberately
overriding the indexed driver. The `hwm14_explicit_input` example demonstrates
the low-level reproducibility path.

## Scientific provenance

The Rust tests check 17 official height points and 19 latitude points, including
both poles, in Quiet and total-wind modes. The pinned upstream output, tolerance,
and offline unmodified-Fortran replay command are documented in
[data/README.md](data/README.md). The original single-height regression remains.

The implementation uses official release `HWM14.123114` and reports the archive
and coefficient-set SHA-256 identities with every result. Please cite:

> Drob et al. (2015), An update to the Horizontal Wind Model (HWM): The quiet
> time thermosphere, Earth and Space Science, doi:10.1002/2014EA000089.

The official supplemental archive does not contain a standalone software license.
Redistribution clearance remains open, including the coefficient assets and
generated upstream-derived adapter. The article license has not been established
as the software license. See the [third-party notices](../../../THIRD_PARTY_NOTICES.md#hwm14--redistribution-review-remains-open)
before public redistribution. Original IonoRay code uses the workspace MIT/Apache-2.0
choice; that choice does not relicense the upstream materials.

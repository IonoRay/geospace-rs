# ionoray-iri

Independent Rust API for the International Reference Ionosphere.

## IRI-2020 backend

The default `iri2020` feature pins the official 25 September 2025 IRI-2020
snapshot. Cargo verifies the release archive and every consumed source,
coefficient, and IGRF file by SHA-256, generates a small explicit-path
`ISO_C_BINDING` adapter, and compiles a local static Fortran library in
`OUT_DIR`. The pinned archive is optional: cache-snapshots retains it locally;
main and source packages exclude it. Supply verified explicit sources/cache or
permit fixed build-time acquisition. `IRI-LICENSE.txt` accompanies the crate.

```bash
nix develop .#default --command cargo build -p ionoray-iri
```

For an already downloaded release, provide either the extracted parent or its
flat `IRI-zip/` directory:

```bash
IONORAY_IRI2020_SOURCE_DIR=/path/to/IRI-zip \
  nix develop .#default --command cargo build -p ionoray-iri
```

To forbid network fallback:

```bash
IONORAY_OFFLINE=1 \
  nix develop .#default --command cargo build -p ionoray-iri
```

Resolution order is:

1. `IONORAY_IRI2020_SOURCE_DIR`;
2. a complete verified source already present in Cargo `OUT_DIR`;
3. a verified archive from `IONORAY_CACHE_ROOT` or the local optional cache;
4. when permitted and no verified source remains, the fixed [official IRI-2020 archive](https://irimodel.org/IRI-2020/).

An existing selected cache must pass its hash check even when OUT_DIR is reusable.
A fresh offline build without a verified source fails explicitly; see
[build sources](../../../docs/releases.md#build-time-sources).

`IONORAY_IRI2020_OFFLINE=1` remains available when only this backend should be
forced offline; `IONORAY_OFFLINE=1` applies to every integrated model build.

## Explicit model boundary

`IriInput` contains a `QueryPoint` and four caller-supplied empirical drivers:
Rz12, IG12, daily adjusted F10.7, and centered 81-day adjusted F10.7. The model
does not read the mutable `ig_rz.dat` or `apf107.dat` files. IG12 is finite but
may legitimately be negative during low solar activity; the other three drivers
are finite and non-negative. Magnetic-storm,
drift, spread-F, auroral-boundary, and sporadic-E extensions are disabled in
this point API because those extensions require additional time-series inputs.
The internal NRLMSIS00 neutral-atmosphere magnetic response is also explicitly
disabled (`SWMI(9)=0`). This is not an Ap=0 scenario and cannot represent
temperature changes driven by geomagnetic activity. No Ap is inferred from
the four solar/ionospheric drivers.

The fixed switches select the following IRI-2020 climatology:
URSI foF2, IRI-cor2 topside (`JF(29)=true`, `JF(30)=false`), Shubin-COSMIC hmF2, ABT-2009 B0/B1, TBT-2012
electron temperature, RBV-2010/TBT-2015 ion composition, and Tru-2021 ion
temperature. Ion species are returned as absolute number densities in m^-3.

NeQuick would require both switches 29 and 30 to be false. The switch table and
`itopn` selection in the pinned `irisub.for` are authoritative.

Replay the single-point numerical reference independently of the Rust adapter:

```bash
nix develop .#default --command python3 scripts/verify_iri2020_reference.py --archive /absolute/cache-root/crates/models/iri/cache/iri2020.tar
```

This compiles unmodified, SHA-256-verified upstream sources in a temporary
directory and calls `IRI_SUB` with the same explicit drivers and switches.
The verifier fills upstream's index COMMON block with **synthetic missing-Ap
sentinels**, selecting its own `SWMI(9)=0` branch without modifying the source.
These are not observations or zero Ap. The production adapter instead removes
the `APFMSIS` lookup, initializes `IAPO`, and sets `SWMI(9)=0` directly. It also
initializes the unused date index and daily-Ap output sentinel so that skipping
`APF_ONLY` does not leave `ISDATE` unset.

At 2020-03-20 12 UTC, 0 N / 0 E, 300 km, Rz12=IG12=10 and both F10.7=70 sfu,
the independent run supplies [20 reference fields](data/reference-no-ap.json):
`OUTF(1:11,1)`, `OARR(1:6)`, `OARR(23,25,27)` in that order. These are number
densities in m^-3, temperatures in K, alternating peak density/height (km), and
three angles (degrees). The Rust test checks all fields, including a negative
cluster-ion sentinel mapping to `None`. The relative tolerance remains 1e-6
(absolute 1e-6 near zero); upstream uses single precision. The original four
reference values are unchanged. This is adapter/reference evidence for one configuration,
not observational validation or coverage of all switches and locations.

Evaluation supports 1958 through 2030 and the common 60 through 1500 km plasma
domain. Components unavailable at a particular altitude, time, or location are
returned as `None`; the wrapper never converts IRI's negative missing-value
sentinels into physical values.
At 60 km, for example, Ne is unavailable because the upstream lower Ne limit
is at least 65 km, even though temperatures can be present. Backend non-finite
outputs return an explicit error; missing values are not replaced with zero.

The verified coefficient set is embedded in the Rust artifact and materialized
atomically under the platform temporary directory. A generated adapter passes
that path explicitly to Fortran. Evaluation is serialized because the upstream
implementation uses common blocks and saved arrays. Runtime evaluation performs
no network, Turso, process-current-directory mutation, or `IONORAY_HOME` access.

## Scientific provenance and license

Every result reports the official release archive hash, coefficient-set hash,
build source, and Rust implementation version. Please cite:

> Bilitza et al. (2022), The International Reference Ionosphere Model: A Review
> and Description of an Ionospheric Benchmark, Reviews of Geophysics 60(4),
> e2022RG000792, doi:10.1029/2022RG000792.

The upstream permission and warranty notice is preserved in
[`IRI-LICENSE.txt`](IRI-LICENSE.txt). The IRI Working Group asks to be
acknowledged in scientific papers that use the software.

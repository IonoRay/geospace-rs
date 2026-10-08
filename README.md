# IonoRay Geospace

Rust libraries, a JSON/JSONL CLI, and Python bindings for geospace empirical
models and the geophysical indices that drive them.

The current implementation provides **IGRF-14**, **IRI-2020**, **HWM14**, and
**NRLMSIS 2.1**. IGRF uses native Rust synthesis; the other three models compile
pinned upstream Fortran sources. Model APIs accept explicit inputs. The
`ionoray-geospace` assembly layer can resolve missing drivers from index data
and retain the actual inputs, source identities, and model provenance.

This is an early research implementation. Numerical reference checks cover
documented cases; they do not establish accuracy for every location, epoch,
or observed event. See [validation](docs/validation.md) for the evidence and limits.

## License and upstream materials

Original IonoRay code is dual licensed under [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE). Bundled model sources, coefficients, index data,
and upstream-derived material have separate terms; see [LICENSE](LICENSE) and
[third-party notices](THIRD_PARTY_NOTICES.md) before using or redistributing them.

In particular, NRLMSIS 2.1 restricts use to research, academic, and non-profit
purposes, and the GFZ snapshot includes sunspot data under CC BY-NC 4.0.
HWM14 software redistribution terms remain an open release-review item.
The `standard`/`cli-standard` features and Python bindings include these backends;
the complete distribution is not covered solely by MIT/Apache-2.0.

## Quick start

Run commands from the repository root. [Nix](https://nixos.org/download/) and
the checked-in `flake.nix`/`flake.lock` define the development environment,
including Rust, gfortran, and Python. Initial setup needs access to Nix packages
and Cargo dependencies; `--locked` preserves the dependency versions.

Start with IGRF-14, which requires neither an index store nor a Fortran backend:

```bash
nix develop .#default --command \
  cargo run --locked -p ionoray-igrf --example igrf14_evaluate
```

The example evaluates UTC 2025-01-01 at WGS84 latitude 30°, longitude 120°,
and ellipsoidal height 300 km. It prints JSON with the input, magnetic field,
angles, and coefficient digest. Field components are east/north/up; the example
also reports nT. See the [Rust guide](docs/rust-models.md) for units and expected values.

After reviewing the upstream terms, run all four explicit-input models:

```bash
nix develop .#default --command env IONORAY_OFFLINE=1 \
  cargo run --locked -p ionoray-geospace --no-default-features \
  --features igrf,iri,hwm,msis --example direct_models
```

`IONORAY_OFFLINE=1` selects the bundled model build assets. Once dependencies
are cached, add Cargo `--offline` to prevent dependency downloads. Runtime index
HTTP access is controlled separately by `DataPolicy`, `SyncMode`, or `SyncPolicy`.
The direct example supplies every driver and does not open an index store.

## Choose an interface

| Need | Entry point | Guide |
|---|---|---|
| Explicit model inputs | Rust `Model::evaluate`, `direct_models` | [Rust models](docs/rust-models.md) |
| Resolve missing drivers and inspect their provenance | `Geospace::prepare_*`, `automatic_drivers` | [Rust models](docs/rust-models.md) |
| Change drivers on a fixed baseline | `Prepared*`, `driver_scenarios` | [Rust models](docs/rust-models.md) |
| Python functions, Session, and Prepared objects | `ionoray_geospace`, `examples/driver_scenarios.py` | [Python models](docs/python-models.md) |
| Process JSON or JSONL requests | `geospace model run` / `model batch` | [CLI records](docs/cli-models.md) |
| Inspect or maintain Kp/ap/Ap, F10.7, IRI indices, Dst, and AE | `indices_offline`, `indices_maintenance`, `geospace indices` | [Data maintenance](docs/data-maintenance.md) |

Automatic-driver and scenario examples use isolated temporary homes and a fixed
offline snapshot. Missing values remain explicit errors or `None`; they are not
replaced with zero. An offline snapshot is not current space-weather data.
The packaged snapshot covers complete GFZ years through 2025, IRI IG/Rz through
2026, and IRI AP/F10.7 through 2024.
Online maintenance requires a selected home and year/range, and writes persistent data.

## Development and documentation

- [Documentation index](docs/README.md): usage, architecture, assets, and future plans.
- [Contributing](CONTRIBUTING.md): build, tests, Clippy, documentation, and debugging.
- [Architecture](docs/architecture.md): independent data/model layers, storage, tracing, and backend packaging.
- [Validation](docs/validation.md): reproducible numerical references and their scientific limits.

The `.vscode` configurations reuse the examples and tests. Select the flake with
`Nix-Env: Select Environment`, reload, and choose a matching Run and Debug launch.
Terminal test results and GUI breakpoint behavior are separate checks.

Runtime home resolution is an explicit absolute path, then `IONORAY_HOME`, then
`~/.ionoray/geospace-rs`. If legacy data exists under `~/.ionoray/indices` or
`~/.ionoray/objects`, implicit opening asks for a root choice instead of migrating
or replacing data. See [data maintenance](docs/data-maintenance.md).

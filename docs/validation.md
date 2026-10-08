# Validation and scientific limits

The repository includes numerical references with pinned source/asset identities
and reproducible replay scripts. These check implementation behavior for the
listed inputs. They are not observations, global accuracy estimates, or evidence
that a new scientific use is valid. A script or test being present does not mean
it has been run on a particular checkout or platform.

## Model references

| Model | Reference coverage | Comparison | Source and limits |
|---|---|---|---|
| IGRF-14 | Nine official Fortran cases across epochs, harmonic degrees, altitude, and poles | 0.5 nT for integer-rounded field values; 1/120° for rounded angles | [IGRF reference](../crates/models/igrf/data/README.md) |
| HWM14 | 17 height points and 19 latitude points, Quiet and total-wind modes | 0.002 m/s per component | [HWM reference](../crates/models/hwm/data/README.md) |
| IRI-2020 | One fixed point in the unmodified upstream no-Ap-response branch | 20 reference fields, relative/near-zero absolute tolerance 1e-6 | [IRI reference](../crates/models/iri/README.md#explicit-model-boundary) |
| NRLMSIS 2.1 | One official Daily row and one independent StormTime call | 12 outputs per point, relative tolerance 5e-12 | [MSIS reference](../crates/models/msis/data/README.md) |

Replay the local, pinned Fortran material from the repository root:

```bash
nix develop .#default --command python3 scripts/verify_hwm14_reference.py
nix develop .#default --command python3 scripts/verify_iri2020_reference.py
nix develop .#default --command python3 scripts/verify_nrlmsis21_reference.py
```

IGRF replay needs a separately obtained local copy of the official `igrf14.f`:

```bash
nix develop .#default --command \
  python3 scripts/verify_igrf14_reference.py /path/to/igrf14.f
```

The scripts verify source hashes, compile in temporary directories, and compare
the committed references. They do not download inputs or call the Rust model
implementation to generate its own expected answers. The model Rust tests then
compare evaluations with those references. See [contributing](../CONTRIBUTING.md)
for the workspace checks.

## Workflow checks

- `crates/geospace/tests/automatic_drivers.rs` and the model-specific workflow
  tests cover missing-driver preparation, explicit overrides, evidence retention,
  and evaluation after the store is closed.
- `crates/geospace/tests/driver_scenarios.rs` covers fixed-baseline changes.
- `crates/geospace/tests/cli_records.rs` exercises the real CLI, record failures,
  continuation, feature availability, and stdout/stderr/exit-code behavior.
- `python/tests/` checks Python results against the same-input Rust interfaces,
  argument validation, Session lifecycle, and scenarios.

Cross-interface comparisons establish consistency; independent Fortran references
provide a separate numerical comparison. Neither proves empirical accuracy.
Fixtures and tests using synthetic failures or a fixed offline snapshot should
not be described as live-provider or real-event validation.

## Applying the models

Inputs use UTC and WGS84 geodetic position with ellipsoidal height. JSON results
use SI units unless a field explicitly says otherwise. IRI uses adjusted F10.7;
MSIS uses observed F10.7 and distinguishes Daily from full StormTime Ap history.
IRI's current point interface disables internal magnetic response; it is not an
Ap=0 scenario or a magnetic-storm temperature prediction.

Missing data and upstream missing-value sentinels remain visible as errors or
`None`. Offline snapshot coverage is finite; Dst and AE need previously synced
local data. A scenario is a controlled sensitivity calculation and does not by
itself establish a physical cause. Consult each model's input domain and original
scientific references before applying it to a research question.

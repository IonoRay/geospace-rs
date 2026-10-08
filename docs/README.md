# Documentation

All runnable commands assume the repository root and the development environment
in `flake.nix`. Start with the [project README](../README.md).

## Usage

| Guide | Contents |
|---|---|
| [Rust models](rust-models.md) | Explicit inputs, automatic drivers, fixed-baseline scenarios, units, examples, and breakpoints |
| [Python models](python-models.md) | Building the extension, direct functions, Session/Prepared lifecycle, errors, and batch analysis |
| [CLI model records](cli-models.md) | JSON/JSONL requests, isolated offline fixtures, result records, and exit codes |
| [Data maintenance](data-maintenance.md) | Home selection, offline/online policies, synchronization, coverage gaps, and recovery |

## Implementation and evidence

- [Architecture](architecture.md): crate dependency direction, CAS/store lifecycle, tracing, and backend packaging.
- [Validation](validation.md): pinned reference inputs, tolerances, independent replay, and scientific limits.
- [Contributing](../CONTRIBUTING.md): development setup, checks, and change expectations.
- [Third-party notices](../THIRD_PARTY_NOTICES.md): license boundaries and outstanding redistribution questions.
- [Index snapshot](../crates/indices/cache/README.md): bundled data provenance and coverage.
- Model details: [IGRF coefficients](../crates/models/igrf/data/README.md),
  [IRI](../crates/models/iri/README.md), [HWM](../crates/models/hwm/README.md),
  and [MSIS](../crates/models/msis/README.md).

## Future work

[Model roadmap](model-roadmap.md) and [WMM plan](wmm-plan.md) describe proposed
work. They are not implemented APIs or evidence of model availability.

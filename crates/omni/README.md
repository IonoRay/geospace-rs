# IonoRay OMNI

One-minute HRO2/HRO CDF downloads and offline solar-wind/IMF reads. Uses
`cdf` (cdf-rs) 0.1.4 directly, without a fork, general CDF wrapper, or native
CDF library. Data lives under `<IONORAY_HOME>/omni/` using the existing scoped
store, CAS, and source provenance machinery.

Every online download rechecks HRO2 and upgrades a previously selected HRO
after validation. Automatic selection never downgrades an accepted local HRO2
on upstream absence. Reports expose the exact product, cadence, version, digest,
before/after identities, selection reason, and upstream-check state. Offline
reads use the same month resolver without making network requests.

See [OMNI usage and validation](../../docs/omni.md) for CLI commands, actual
fields, product fallback, missingness, tests, and limitations.

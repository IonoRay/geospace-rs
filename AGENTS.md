# geospace-rs working agreement

- `flake.nix` is the authoritative development toolchain. Run project commands through `nix develop .#default --command ...`.
- Preserve the dependency direction documented in `docs/architecture.md`; data and model crates must remain independent.
- Model crates accept explicit inputs and must not access Turso, the network, or `IONORAY_HOME`.
- Keep production Rust source files below 500 lines. Split by responsibility before reaching the limit.
- Do not expose placeholder scientific results. Unimplemented model evaluation must return an explicit error.
- Every behavior change needs focused tests and must pass format, Clippy, tests, and documentation checks.

# Contributing

Use the repository-root `flake.nix` development environment. The lockfiles
define the current toolchain and dependencies; do not update them as a side
effect of an unrelated change. On a fresh checkout, first populate the Nix
environment and Cargo cache with a normal `--locked` build. The commands below
use cached dependencies and packaged model build assets.

Read [architecture](docs/architecture.md) and [third-party notices](THIRD_PARTY_NOTICES.md)
before changing model backends or redistributing artifacts. Model crates take
explicit inputs and must remain independent of indices, Turso, HTTP access,
and `IONORAY_HOME`. Retain units, missing values, and source identities.

## Rust checks

```bash
nix develop .#default --command cargo fmt --all -- --check
nix develop .#default --command env IONORAY_OFFLINE=1 \
  cargo clippy --locked --offline --workspace --all-targets --all-features --no-deps -- -D warnings
nix develop .#default --command env IONORAY_OFFLINE=1 \
  cargo test --locked --offline --workspace --exclude ionoray-python --all-features
nix develop .#default --command env IONORAY_OFFLINE=1 \
  cargo test --locked --offline -p ionoray-python
nix develop .#default --command env IONORAY_OFFLINE=1 RUSTDOCFLAGS=-Dwarnings \
  cargo doc --locked --offline --workspace --all-features --no-deps
git diff --check
```

Test the Python Rust crate without `extension-module`: that feature is for
building the loadable extension, rather than linking Rust test executables.
For a scoped change, run the focused tests first. Every behavior change needs
tests for the normal case and relevant boundaries, followed by applicable
format, Clippy, test, and documentation checks. Keep production Rust source
files below 500 lines and return explicit errors for unavailable model evaluation.
Online tests marked ignored remain opt-in; do not run historical synchronization
just to check an example.

## Python and numerical references

Python 3.10 or newer is required. Create a local environment once:

```bash
nix develop .#default --command python3 -m venv .venv
```

The [Python guide](docs/python-models.md#构建与运行) builds the extension with
the flake-provided maturin, prepares its Rust comparison executable, and runs
the unittest suite. The preparation script requires an existing `.venv`; it
does not install network dependencies. See [validation](docs/validation.md)
for the independent Fortran replay commands and documented tolerances.

## Debugging and changes

Use the checked-in `.vscode` launches or a focused Rust `#[test]` CodeLens.
The [Rust](docs/rust-models.md), [CLI](docs/cli-models.md), and
[data](docs/data-maintenance.md) guides identify inputs, outputs, and breakpoints.
CLI stdout is reserved for result records; diagnostics use stderr.
Record terminal verification and editor breakpoint checks separately.

Describe the concrete behavior change, the input that demonstrates it, the
checks run, and any unverified scientific or platform limits in a pull request.
Numerical agreement with a reference implementation is not observational validation.
Do not commit runtime stores, generated binaries, virtual environments, or credentials.

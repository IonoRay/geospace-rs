# Scientific model roadmap

## Purpose

This document defines the planned expansion of `crates/models` beyond the
currently validated IGRF-14, IRI-2020, HWM14, and NRLMSIS 2.1 implementations.
It is a prioritization and acceptance plan, not a commitment to expose
unvalidated scientific output.

The roadmap favors independently useful empirical or semi-empirical models that
can run synchronously from explicit inputs. Large time-dependent simulation
systems are intentionally kept outside the model-crate boundary.

## Non-negotiable boundaries

Every new model must preserve the dependency direction in
[`architecture.md`](architecture.md):

- model crates depend on `ionoray-core`, never on `ionoray-indices`, Turso, the
  network, or `IONORAY_HOME`;
- environmental observations are explicit model inputs;
- `ionoray-geospace` is the only layer that may resolve missing inputs from data
  products and construct a reusable prepared input;
- an unavailable or incomplete backend returns an explicit error and never a
  placeholder scientific value;
- upstream source, coefficients, test vectors, license, and publication policy
  are reviewed before implementation begins;
- upstream artifacts with incompatible or unclear redistribution terms are not
  vendored into the repository or published crate;
- every backend has official or independently reproducible reference-value tests,
  provenance, focused tracing, and a debug example;
- production Rust files remain below 500 lines, with 300 lines as the normal
  responsibility-splitting point.

## Shared prerequisites

### Coordinate and field-line infrastructure

Magnetospheric models require geocentric and solar-magnetic frames that do not
belong to any one model. Add an independent crate, provisionally
`ionoray-coordinates`, before exposing an external magnetic-field model. Its
initial scope is:

- typed positions and vectors in GEO, GEI, GSE, GSM, SM, and MAG frames;
- epoch-dependent, explicitly documented transformations;
- composition with the existing WGS84 geodetic and IGRF APIs;
- deterministic field-line integration with explicit tolerances, step limits,
  and terminal surfaces;
- reference tests at coordinate singularities, poles, equinoxes, and solstices.

Coordinate transforms and numerical field-line tracing are infrastructure, not
scientific model families, and therefore should not live under `crates/models`.

### Solar-wind data product

T96, magnetospheric boundary, electrodynamic, and auroral models require solar
wind and IMF inputs that are not in the current Kp/Ap/F10.7/Dst/AE product. Add a
source-specific OMNI data product before offering automatic preparation for
these models. At minimum it should preserve:

- solar-wind velocity and proton density;
- dynamic pressure, either observed or derived with recorded provenance;
- IMF By and Bz in the frame required by the model;
- source cadence, quality flags, propagation convention, and exact selected
  samples.

The model APIs must still accept these quantities directly and remain usable
without the data layer.

### License gate

"Public source" is not equivalent to "redistributable dependency." Before a
candidate moves into implementation, record:

1. the authoritative source and immutable release identity;
2. source, coefficient, and reference-output hashes;
3. modification and redistribution permissions;
4. citation and publication requirements;
5. whether automatic download, a caller-supplied directory, and offline builds
   are permitted.

When redistribution is unclear or incompatible with the workspace license, use
the verified external-source pattern established by IRI, HWM, and NRLMSIS only
if the upstream terms permit it.

## Prioritized roadmap

### P0: magnetospheric field and boundaries

#### `ionoray-tsyg`: Tsyganenko external magnetic field

Implement T89d first, then T96 after the solar-wind data product exists.

- **T89d inputs:** epoch, GSM position, and Kp activity bin.
- **T96 inputs:** epoch, GSM position, solar-wind dynamic pressure, Dst, IMF By,
  and IMF Bz.
- **Outputs:** external magnetic-field vector with model version, coordinate
  frame, input provenance, and validity metadata.
- **Why first:** it fills the largest current domain gap and T89d can reuse the
  existing Kp product.
- **Entry gate:** confirm redistribution terms for the authoritative
  Tsyganenko/GEOPACK source. If they remain unclear, do not publish upstream
  source inside the crate.
- **Reference sources:** [official GEOPACK distribution](https://geo.phys.spbu.ru/~tsyganenko/empirical-models/coordinate_systems/geopack/)
  and [NASA CCMC instant-run interface](https://ccmc.gsfc.nasa.gov/ror/requests/instant/tsyganenko.php).

T89d acceptance requires field-vector comparisons across all Kp bins, both
hemispheres, dayside and magnetotail positions, and dates with different dipole
tilt. T96 additionally requires quiet and storm-time cases spanning its input
domain.

#### `ionoray-boundaries`: magnetopause and bow shock

Start with the Shue et al. 1998 magnetopause and add a separately versioned
Farris-Russell bow-shock model only after its equations and validity domain have
been independently checked.

- **Inputs:** GSM direction, solar-wind dynamic pressure, IMF Bz, and any
  model-specific Mach number.
- **Outputs:** boundary radius, Cartesian surface point, inside/outside
  classification, and validity metadata.
- **Implementation:** native Rust equations with no hidden global state.
- **Use:** terminal surfaces for field-line tracing and an explicit domain check
  for magnetospheric models.
- **Reference source:** [NASA CCMC magnetopause validation challenge](https://ccmc.gsfc.nasa.gov/challenges/gem-magnetopause/).

Boundary models must expose their empirical validity domains. Extrapolation may
be allowed only through an explicit policy and must be reported in the result.

### P1: ionosphere and plasmasphere

#### `ionoray-nequick`: NeQuick-G

NeQuick-G adds Galileo-compatible ionospheric delay and slant-TEC calculations
that are complementary to IRI point evaluation.

- **Inputs:** epoch, receiver/satellite geometry, broadcast ionospheric
  coefficients, and model-defined solar activity terms.
- **Outputs:** electron-density profile contributions, slant TEC, and group
  delay with the frequency and integration policy recorded.
- **License:** the JRC implementation is published under EUPL-1.2; compatibility
  with the workspace distribution must still be documented explicitly.
- **Reference source:** [European Union Agency for the Space Programme](https://www.euspa.europa.eu/newsroom-events/news-archive/nequick-g-code-available-download).

Acceptance requires official Galileo test cases, independent numerical
integration checks, path-reversal tests where applicable, and explicit behavior
at horizon and invalid-geometry boundaries.

#### `ionoray-gcpm`: Global Core Plasma Model 2.4

GCPM extends density and composition from the topside ionosphere into the
plasmasphere, plasmapause, trough, and polar cap.

- **Inputs:** location in the model's magnetic coordinates, epoch, Kp, and all
  explicitly required solar/ionospheric drivers.
- **Outputs:** total electron density and H+, He+, and O+ composition with region
  classification.
- **Integration constraint:** preserve the official GCPM 2.4 reference behavior
  first. Do not silently replace its historical IRI coupling with IRI-2020 and
  call the result GCPM 2.4; any modernized coupling must be a separately named,
  validated configuration.
- **Reference source:** [NASA plasmaspheric model archive](https://plasmasphere.nasa.gov/models/).

### P2: upper-atmosphere radiation and drag ensembles

#### `ionoray-glow`: NCAR GLOW

GLOW is the highest-value coupling model after the neutral-atmosphere and
ionosphere foundations are stable. It consumes neutral and ionospheric profiles
plus solar EUV or auroral precipitation and produces photoelectron, ionization,
excitation, and optical-emission profiles.

- **Inputs:** explicit altitude grids, neutral atmosphere, ionosphere, solar EUV
  spectrum, and optional auroral electron specification.
- **Outputs:** energetic-electron distributions, production rates, and volume
  emission rates; column brightness belongs in a separate post-processing API.
- **License:** academic research terms require an external-source and publication
  policy review before implementation.
- **Reference source:** [NSF NCAR High Altitude Observatory](https://www2.hao.ucar.edu/modeling/glow).

The first implementation should accept caller-supplied MSIS and IRI profiles. It
must not query or invoke those crates implicitly inside the GLOW model crate.
Profile assembly belongs in `ionoray-geospace`.

#### `ionoray-jb`: JB2008

JB2008 provides an independent thermospheric-density estimate for orbit-drag
comparison and model ensembles.

- **Inputs:** F10, S10, M10, Y10, Ap, Dst, and DTC with the exact averaging and
  lag conventions defined by the model.
- **Data gap:** S10, M10, Y10, and DTC require new source-specific ingestion and
  provenance; existing F10.7/Ap/Dst coverage is insufficient.
- **Packaging gate:** audit the downloaded code and data terms before choosing a
  native port or external-source backend.
- **Reference source:** [official JB2008 code, inputs, and validation files](https://spacewx.com/jb2008/).

Do not add JB2008 until its official validation case can be reproduced and every
nonstandard driver can be supplied explicitly.

#### Conditional: `ionoray-dtm` for DTM2020

DTM2020 operational mode is technically attractive because it uses F10.7 and Kp,
already present in the data product, and returns thermospheric temperature,
total density, partial densities, and uncertainty. It is not an unconditional
roadmap item because the published license restricts the software to academic,
non-commercial use and prohibits modification and dissemination.

Do not implement or distribute DTM2020 by default unless a license review
confirms the intended build, publication, and user workflows. If approved, it
must be an opt-in external-source backend rather than vendored code.

- **Reference source and license:** [SWAMI MCM/DTM2020 repository](https://github.com/swami-h2020-eu/mcm).

### P3: high-latitude electrodynamics and aurora

#### `ionoray-weimer`: Weimer 2005

This model provides high-latitude electric potential and derived electric-field
quantities. It depends on solar-wind/IMF state and therefore follows the OMNI
data product and coordinate infrastructure.

- **Inputs:** model-defined solar-wind speed, IMF magnitude/orientation, dipole
  tilt, and optional activity terms.
- **Outputs:** potential and electric field in an explicit magnetic frame.
- **Entry gate:** obtain an authoritative source package, coefficient manifest,
  reference outputs, and clear redistribution terms.
- **Reference publication:** [Weimer 2005](https://doi.org/10.1029/2004JA010884).

#### `ionoray-ovation`: OVATION Prime

OVATION Prime supplies statistical auroral electron and ion precipitation for
four precipitation categories.

- **Inputs:** solar-wind velocity, density, and IMF By/Bz with the official
  coupling and time-window definitions.
- **Outputs:** energy flux and characteristic energy on an explicit magnetic
  latitude/local-time grid.
- **Packaging gate:** the authoritative implementation is IDL and requires a
  license, coefficient, and reference-case audit before any Rust port.
- **Reference source:** [NASA CCMC OVATION Prime](https://ccmc.gsfc.nasa.gov/models/Ovation-Prime~1.0/).

## Systems that do not belong in `crates/models`

TIE-GCM, WAM-IPE, CTIPe, SWMF, MAGE, RCM, and similar systems are
time-dependent grid solvers with substantial initial conditions, boundary
conditions, coupling, and operational runtime requirements. Wrapping them as
synchronous point models would hide their scientific state and violate the
current architecture.

If support is needed later, add separate simulation-runner and gridded-output
adapter layers. A point query may interpolate a completed, provenance-addressed
simulation product, but it must not present that result as execution of an
independent model crate. See the [NASA CCMC model catalog](https://ccmc.gsfc.nasa.gov/models/)
for the distinction between instant empirical models and runs-on-request
simulation systems.

## Definition of done for every model

A roadmap item is complete only when all of the following are true:

1. **Scientific contract:** release, input units, coordinate frames, valid
   domain, switches, missing values, and outputs are represented explicitly.
2. **Source contract:** authoritative artifacts are pinned and hash-verified;
   license, citation, and publication requirements are included in user-facing
   documentation.
3. **Architecture:** the model runs synchronously and offline from explicit
   inputs, with no data-store or environment discovery inside the model crate.
4. **Safety:** FFI is isolated behind a safe Rust API, upstream global state is
   serialized, inputs are validated before crossing the ABI, and failures are
   typed errors.
5. **Verification:** official reference vectors and independent edge cases cover
   the supported domain; tolerances and units are justified.
6. **Provenance:** every result records model version, source/coefficient digest,
   requested switches, and any extrapolation or degraded mode.
7. **Operability:** a focused example and VS Code LLDB entry support step-through
   debugging without hidden preparation.
8. **Package boundary:** `cargo package --list` confirms that no forbidden
   upstream source or data is shipped.
9. **Quality gates:** the Nix flake check, format, workspace Clippy with warnings
   denied, tests, documentation, and `git diff --check` all pass.

## Next milestone: T89d vertical slice

The next implementation should stop after a reviewable T89d vertical slice:

1. resolve and document the official source and redistribution boundary;
2. define frame-typed vector APIs and implement the minimum GEO/GEI/GSM/SM/MAG
   transformations required by GEOPACK;
3. expose T89d as an offline explicit-input model with Kp-bin validation;
4. compare field vectors with official Fortran/CCMC results across the acceptance
   matrix;
5. add deterministic field-line tracing using IGRF plus T89d and a configurable
   terminal surface;
6. integrate optional Kp preparation only in `ionoray-geospace`;
7. add provenance, tracing, a debug example, documentation, and complete quality
   gates.

T96, OMNI ingestion, automatic magnetopause termination, and additional
Tsyganenko releases remain separate follow-up increments. They must not expand
the first milestone before T89d reference parity is established.

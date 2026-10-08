# NRLMSIS 2.1 numerical references

[`reference.json`](reference.json) contains two point evaluations from the
unmodified, pinned official NRLMSIS 2.1 Fortran source compiled with `-DDBLE`:

| Case | UTC / WGS84 position | Solar drivers (sfu) | Ap formulation |
|---|---|---|---|
| `daily_official_row_74213` | 1974-08-01 10:58:30, -24.4°, 119.1°, 399.1 km | F10.7a 86.5, previous-day F10.7 84.8 | Daily Ap 6 |
| `storm_time_offline_snapshot` | 2020-07-01 12:00:00, 30°, 120°, 300 km | observed F10.7a 69.9543209876543, previous-day observed F10.7 68.1 | `[4, 2, 4, 3, 4, 3.375, 2.5]` |

The output order is temperature, exospheric temperature, then the new
`MSISCALC` density array: total mass density, N2, O2, O, He, H, Ar, N,
anomalous O, and NO. Temperatures are K, mass density is kg/m3, and number
densities are m-3. Missing densities use the upstream `9.999e-38` sentinel and
the Rust wrapper maps them to `None`; neither reference point has a missing
density.

The Daily input is upstream test record 74213. The verifier checks its direct
new-interface result against the archived `msis2.1_test_ref_dp.txt` row after
converting the legacy g/cm3 and cm-3 columns to SI. That official output file's
SHA-256 is
`59210a442f175b6b3f9e15034856989eea46d54ddb4fe3057a289a77512b7ce3`.

The official package has no StormTime output table. The second reference is an
independent direct call to the unmodified source with
`switch_legacy(9) = -1`; it does not call the Rust adapter or automatic-driver
path. Its seven Ap values are independently transcribed and averaged from GFZ
rows 32363–32365 in the bundled snapshot. This establishes the StormTime
numerical mode but does not constitute observational validation.

Replay both cases offline from the repository root:

```bash
nix develop .#default --command python3 scripts/verify_nrlmsis21_reference.py
nix develop .#default --command env IONORAY_OFFLINE=1 \
  cargo test --locked --offline -p ionoray-msis --lib
```

The replay checks the official archive hash, compiles in a temporary directory,
and compares all 12 outputs per case with a relative tolerance of `5e-12`.
The Rust tests use the same tolerance because both paths use the official
double-precision formulation. These two points do not cover the whole altitude,
time, solar-activity, or geomagnetic-activity domain.

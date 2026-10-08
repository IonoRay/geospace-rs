# IGRF-14 coefficients

`igrf14coeffs.txt` is the official IAGA IGRF-14 coefficient table distributed
by NOAA/NCEI:

- source: <https://www.ngdc.noaa.gov/IAGA/vmod/coeffs/igrf14coeffs.txt>
- model release: IGRF-14, finalized November 2024
- retrieved: 2026-07-12
- SHA-256: `8f8d88403028fc4ee92c4f38d97b46e0a87e2cfc496045b43c9e26c1d6b0903c`

The file is retained verbatim on cache-snapshots and excluded from main/source
packages. Build acquisition accepts `IONORAY_IGRF14_COEFFICIENT_FILE` or
`IONORAY_CACHE_ROOT`, verifies the fixed hash and embeds the coefficients. Fresh
offline builds without a source fail explicitly; runtime evaluation never fetches
coefficients. Runtime parsing is deterministic and its digest
is included in every evaluation's provenance.

## Reproducible numerical reference (2026-09-23)

The [NCEI model page](https://www.ncei.noaa.gov/products/international-geomagnetic-reference-field)
links to the [BGS Fortran program](https://www.ngdc.noaa.gov/IAGA/vmod/igrf14.f).
The source fetched for this check has SHA-256
`0c4fe0664d837b86c1d1fb4d2a354d6a14a29bbf028b550ab1342e1b828dd2c0`.
The coefficient table was fetched again and matched the pinned digest above.
The reference is an independent implementation comparison, not an observation
of the actual geomagnetic field or a validation of every location/epoch.

`reference.json` contains nine outputs from the unmodified interactive program,
compiled with `gfortran -std=legacy -O0`. The replay script checks the source hash,
compiles/runs in a temporary directory, and compares all fixture fields. It does
not invoke Rust or download anything. From the repository root, given a local
copy of that official source:

```bash
nix develop .#default --command python3 scripts/verify_igrf14_reference.py /path/to/igrf14.f
```

The script supplies screen output, geodetic coordinates (`ITYPE=1`), spot mode,
decimal degrees, date, altitude in km, latitude/longitude, a place label, and
`n` to stop. The upstream driver calls `IGRF14SYN(0, ...)` for the main field.
The integer X/Y/Z/F outputs use `NINT`, so tests allow 0.5 nT; D/I are rounded to
integer arcminutes, so tests allow 1/120 degree. Negative degree values carry
the sign of their positive minute part (e.g. -5 degrees 18 minutes = -5.3 degrees).
No tolerance was widened to match this Rust implementation.

Cases cover 1900/2030 endpoints, degree-10 1990/1994, degree-13 1995/2000,
both poles, altitude, 2020.5 interpolation, 2025 base and 2027 secular variation.
The old named reference tests remain, and `matches_reproducible_official_reference_matrix`
compares all nine cases, including angles and selected harmonic degree.

Reference X/Y/Z are north/east/down, while Rust exposes east/north/up:
`east=Y`, `north=X`, `up=-Z`. At a geographic pole the chosen longitude defines
the local horizontal axes; declination there is not a longitude-independent
physical bearing. Both implementations use the upstream WGS84 ellipsoid
constants and reference radius 6371.2 km. Height is ellipsoidal, not geocentric
radius or height above mean sea level.

The Rust interface supports decimal years [1900, 2030] inclusive; it deliberately
does not use the upstream program's reduced-accuracy extrapolation to 2035.
UTC epoch conversion uses elapsed seconds since January 1 divided by the duration
of that calendar year (including leap days/leap seconds). The fractional fixture
uses 2020-07-02T00:00:00 UTC = 2020.5, with no leap-second ambiguity.

# HWM14 reference profiles

`reference-profiles.txt` is the verbatim first two profiles of
`HWM14/Check/gfortran.txt` from the packaged official NRL release HWM14.123114.
It is an upstream reference, not output recorded from the Rust implementation.

- [Official archive](https://map.nrl.navy.mil/map/pub/nrl/HWM/HWM14/HWM14_ess224-sup-0002-supinfo.tgz)
- Archive SHA-256: `4de451beeadef7b3ec3aa5b91129ea98866b9e7156cecf4be1343c33a6f57978`
- Complete `Check/gfortran.txt` SHA-256: `2b1d4f4f103be3531393c48bf32d034548babfb6c080549884cad9fd3a2c8652`
- Source, coefficient hashes and build identity: [build.rs](../build.rs).

The archived `checkhwm14.f90` defines these inputs:

| Profile | UTC in 1995 | Latitude / longitude (degrees) | Height (km) | ap |
|---|---|---|---|---|
| Height, 17 points | day 150 = May 30, 12:00 | -45 / -85 | 0 to 400, step 25 | 80 |
| Latitude, 19 points | day 305 = November 1, 18:00 | -90 to 90, step 10 / 30 | 250 | 48 |

The geographic meridional/zonal components are positive north/east, in m/s.
`Quiet` compares with `quiet`; Rust `Disturbed` returns total winds and compares
with `total`. The table's `disturbed` column is the DWM07 perturbation alone.
At the poles the components use the specified longitude's local basis.

The fixture prints three decimals. The Rust check retains the existing absolute
tolerance of 0.002 m/s per component, allowing printed rounding and the small
compiler-dependent differences discussed in the archived README. This is a
numerical reproduction tolerance, not a claim of observational accuracy.

From the repository root, replay the unmodified official Fortran driver and
compare its first two profiles with the archive and committed excerpt:

```bash
nix develop .#default --command python3 scripts/verify_hwm14_reference.py
nix develop .#default --command env IONORAY_OFFLINE=1 cargo test --locked --offline -p ionoray-hwm --lib
```

The script uses local assets only, compiles in a temporary directory, and does
not call Rust or the generated adapter. It checks all six official wind columns;
Rust checks Quiet/total (72 evaluations, 144 components). The archived driver
also runs other profiles; the script does not claim to verify those outputs.

These profiles cover surface to 400 km and both poles at two fixed dates/ap
values. They do not validate all seasons, solar times, ap values, or empirical
accuracy. The archived README describes disturbance climatology above 225 km,
attenuated at lower heights; it is not an event-specific wind prediction.

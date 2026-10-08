# Package third-party notices

Original IonoRay code uses MIT OR Apache-2.0. Dependencies retain their own terms.

## NRLMSIS 2.1 — restricted upstream agreement

- Material: `cache/nrlmsis2.1.tar.gz`, compiled upstream
  sources/parameters, the generated adapter, and numerical references.
- Source: [official NRLMSIS 2.1 directory](https://map.nrl.navy.mil/map/pub/nrl/NRLMSIS/NRLMSIS2.1/).
- The exact upstream agreement is copied, without byte changes, to
  [nrlmsis2.1_license.txt](nrlmsis2.1_license.txt). Its original
  archive entry is named `nrlmsis2.1_license..txt`. The embedded build artifact
  exposes the agreement through `nrlmsis21_license_bytes()`.
- The agreement restricts use to research, academic, and non-profit purposes.
  Commercial use and fee-based transfer, including covered data products, need
  the specified written permission. The agreement must accompany software copies.
- Section 4(b) requires prominent notices and delivery to NRL for modifications
  or derivative works. Review how that condition applies to the generated
  adapter before distributing a compiled backend; no delivery to NRL is claimed here.

Required package notice (spelling normalized to match the existing API notice):

> This software incorporates the MSIS empirical atmospheric model software
> designed and provided by NRL. Use is governed by the Open Source Academic
> Research License Agreement contained in nrlmsis2.1_license.txt.

The exact agreement, including its original trademark notation, remains the
authority. See the [MSIS README](README.md#license-boundary).


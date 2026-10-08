# Third-party materials and license boundaries

The [MIT/Apache-2.0 choice](LICENSE) covers original IonoRay code. It does not
replace upstream terms for the software, data, reference extracts, or derived
material listed here. Pure original-code packages use SPDX metadata. Mixed model/data packages use
`license-file` and include their local scope statement and upstream notices.

This inventory records the current materials and unresolved release questions.
It does not claim that all redistribution rights have been cleared. Ordinary
Rust dependencies are identified by `Cargo.lock` and retain their own licenses;
review their notices when distributing compiled artifacts.

## IGRF-14

- Material: `crates/models/igrf/data/igrf14coeffs.txt`, official IAGA coefficients
  distributed by NOAA/NCEI, retained verbatim.
- Source: [official IGRF page](https://www.ncei.noaa.gov/products/international-geomagnetic-reference-field)
  and [coefficient table](https://www.ngdc.noaa.gov/IAGA/vmod/coeffs/igrf14coeffs.txt).
- The embedded coefficient provenance, digest, and numerical reference details
  are in the [data README](crates/models/igrf/data/README.md).
- Cite the model release and coefficient provider in scientific use. The
  coefficient table is not relicensed as original IonoRay software.

## IRI-2020 and IRI index files

- Material: `crates/models/iri/cache/iri2020.tar`, its upstream-derived build
  adapters and references, and `crates/indices/cache/ig_rz.dat` / `apf107.dat`.
- Source: [IRI Working Group](https://irimodel.org/),
  [IRI-2020](https://irimodel.org/IRI-2020/), and
  [IRI indices](https://irimodel.org/indices/).
- Retain the exact [IRI license](crates/models/iri/IRI-LICENSE.txt) with software
  copies. It permits use, copying, and modification subject to the notice and
  scientific acknowledgement conditions.
- Acknowledge the IRI Working Group and cite the paper describing the model
  version. See [IRI provenance](crates/models/iri/README.md#scientific-provenance-and-license).
- The provider describes the indices as part of the IRI software package;
  their format, source identities, and snapshot coverage are retained in the
  [index cache README](crates/indices/cache/README.md). Do not infer a separate
  MIT/Apache data license.

## HWM14 — redistribution review remains open

- Material: `crates/models/hwm/cache/hwm14.tgz`, the generated upstream-derived
  explicit-path adapter, and `data/reference-profiles.txt`.
- Source: [official NRL HWM14 directory](https://map.nrl.navy.mil/map/pub/nrl/HWM/HWM14/).
  The [cache README](crates/models/hwm/cache/README.md) records the archive digest.
- The pinned archive includes author and citation notices but no standalone
  software license. Public download access does not establish a redistribution
  license. Third-party mirrors' license labels do not authorize this archive.
- Cite Drob et al. (2015), *An update to the Horizontal Wind Model (HWM): The
  quiet time thermosphere*, [doi:10.1002/2014EA000089](https://doi.org/10.1002/2014EA000089).
  The [article hosted by NRL](https://map.nrl.navy.mil/map/pub/nrl/HWM/HWM14/HWM14_ess224.pdf)
  states CC BY-NC-ND terms for the article; applicability to the software and
  coefficient supplement, especially the generated adapter, has not been established.
- Before public redistribution, obtain applicable software/asset terms from
  the provider or choose an explicit external-source packaging policy. The pinned archive lives only on local cache-snapshots; build adapters are
  shared source and still require a rights review before public distribution.

## NRLMSIS 2.1 — restricted upstream agreement

- Material: `crates/models/msis/cache/nrlmsis2.1.tar.gz`, compiled upstream
  sources/parameters, the generated adapter, and numerical references.
- Source: [official NRLMSIS 2.1 directory](https://map.nrl.navy.mil/map/pub/nrl/NRLMSIS/NRLMSIS2.1/).
- The exact upstream agreement is copied, without byte changes, to
  [nrlmsis2.1_license.txt](crates/models/msis/nrlmsis2.1_license.txt). Its original
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
authority. See the [MSIS README](crates/models/msis/README.md#license-boundary).

## GFZ geomagnetic and solar index snapshot

- Material: `crates/indices/cache/Kp_ap_Ap_SN_F107_since_1932.txt`, retained verbatim.
- Provider: Geomagnetic Observatory Niemegk, GFZ Helmholtz Centre for Geosciences;
  [official data access](https://kp.gfz.de/en/data).
- The file header declares [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/)
  except for the included sunspot numbers, which use
  [CC BY-NC 4.0](https://creativecommons.org/licenses/by-nc/4.0/).
  Keep the header and attribution; bundling the file does not remove its
  non-commercial sunspot-data restriction.
- Scientific citation: Matzka et al. (2021), *The geomagnetic Kp index and derived
  indices of geomagnetic activity*, [doi:10.1029/2020SW002641](https://doi.org/10.1029/2020SW002641).
  Dataset citation: Matzka et al. (2021), *Geomagnetic Kp index*, V. 1.0,
  GFZ Data Services, [doi:10.5880/Kp.0001](https://doi.org/10.5880/Kp.0001).
- Snapshot date, coverage, and SHA-256 values are in the
  [index cache README](crates/indices/cache/README.md).

Kyoto Dst/AE archives are downloaded only on an explicit online data path and
are not bundled. Their provider's terms still apply to subsequently downloaded
data and any redistribution of a user's local store.

## Native runtime libraries in wheels

Portable wheels must use `maturin build --auditwheel repair` and pass actual
Mach-O/ELF dependency inspection. Original Rust metadata alone does not describe
those copied libraries. `licenses/native/` retains the GNU GPL-3.0, LGPL-2.1 and
GCC Runtime Library Exception 3.1 texts from the GCC upstream repository.
GCC libgfortran/libgcc use the runtime exception; libquadmath and GNU libintl
retain their own library terms. Source and modifications must be reviewed for
the actual runtime version/platform before distributing copied libraries.

The local macOS Nix toolchain also links Apple libiconv 115.100.1/libcharset.
The exact upstream `libcharset/libcharset.c` header identifies APSL-1.0
(`licenses/native/APPLE-libcharset-NOTICE.txt`), while Nix package metadata lists
BSD licenses. That discrepancy is an additional binary release review item;
package metadata is insufficient permission evidence. A macOS binding link
now drops unused dylibs. Base wheels need no Fortran runtime; full-model wheels
remain local research artifacts until all provider/runtime obligations are cleared.

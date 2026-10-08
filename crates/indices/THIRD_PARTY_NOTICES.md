# Package third-party notices

Original IonoRay code uses MIT OR Apache-2.0. Dependencies retain their own terms.

## IRI-2020 and IRI index files

- Material: `crates/models/iri/cache/iri2020.tar`, its upstream-derived build
  adapters and references, and `cache/ig_rz.dat` / `apf107.dat`.
- Source: [IRI Working Group](https://irimodel.org/),
  [IRI-2020](https://irimodel.org/IRI-2020/), and
  [IRI indices](https://irimodel.org/indices/).
- Retain the exact [IRI license](IRI-LICENSE.txt) with software
  copies. It permits use, copying, and modification subject to the notice and
  scientific acknowledgement conditions.
- Acknowledge the IRI Working Group and cite the paper describing the model
  version. See [IRI provenance](crates/models/iri/README.md#scientific-provenance-and-license).
- The provider describes the indices as part of the IRI software package;
  their format, source identities, and snapshot coverage are retained in the
  [index cache README](cache/README.md). Do not infer a separate
  MIT/Apache data license.


## GFZ geomagnetic and solar index snapshot

- Material: `cache/Kp_ap_Ap_SN_F107_since_1932.txt`, retained verbatim.
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
  [index cache README](cache/README.md).

Kyoto Dst/AE archives are downloaded only on an explicit online data path and
are not bundled. Their provider's terms still apply to subsequently downloaded
data and any redistribution of a user's local store.


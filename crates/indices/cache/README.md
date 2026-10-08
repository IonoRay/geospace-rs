# Bundled index snapshot

These optional verbatim snapshots are retained on cache-snapshots; main and
source packages exclude their bytes. An explicit IONORAY_CACHE_ROOT can supply
them at build time. Requested online source checks take precedence; Ensure can
reuse complete local coverage. Offline uses existing CAS/embedded snapshots and
reports gaps when insufficient, without creating an HTTP request.

Snapshot: 2026-07-16

| File | Upstream | Complete years | SHA-256 |
|---|---|---:|---|
| `Kp_ap_Ap_SN_F107_since_1932.txt` | GFZ Potsdam | 1932-2025 | `a74cd1096e7b7711690ffba819ddf7149bf32f07a787092510407e5aa1742029` |
| `ig_rz.dat` | IRI Working Group | 1958-2026 | `48e6ac1a501c39ad9842fcf75d63a86303e09852406ee4153275d4b82f9e8804` |
| `apf107.dat` | IRI Working Group | 1958-2024 | `25bb20ff10c9cc9bf3ec9b80774e856119145d0ce5bbf65dfd3f2bd046ef9262` |

The GFZ file declares CC BY 4.0 for its geomagnetic indices and CC BY-NC 4.0
for contained sunspot numbers. IRI files retain their upstream data format and
provenance; see the project documentation for source links and citations.

Kyoto Dst and AE archives are deliberately not bundled: a complete historical
snapshot would make the crate disproportionately large. Offline Dst/AE access
therefore requires those files to have already been synchronized into the
user's local CAS.

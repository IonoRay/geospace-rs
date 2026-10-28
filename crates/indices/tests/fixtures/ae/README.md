# WDC Kyoto AE real daily fixtures
Retrieved 2026-10-28. Verbatim official E02 files; no generated values.
Used for hourly AE parsing, daily partitions, priority, HTTP fallback and CAS recovery.
AE values remain in nT. Tests use the published hourly mean, not a recomputed minute average.

- Source: https://wdc.kugi.kyoto-u.ac.jp/ae_realtime/data_dir/2024/05/01/ae240501
- File: `ae240501.txt`
- SHA-256: `f4e62df040d144c52325d44f2fdd0912d0e0f8f26d98c08f9c776cc27de66444`

- Source: https://wdc.kugi.kyoto-u.ac.jp/ae_provisional/202405/ae240510.for.request
- File: `ae240510.txt`
- SHA-256: `760bbeec2980749e6ed2e82fd4dedcbc7380f74dfde4511dc618a276150b01f7`

Reference checks read directly from the fixed columns of the original first row:
2024-05-01T00:00Z realtime AE hourly mean = 647 nT;
2024-05-10T00:00Z provisional AE hourly mean = 168 nT.
Each file has 24 hourly rows. The provisional file includes a creation-time trailer.
Upstream use and acknowledgement guidance: https://wdc.kugi.kyoto-u.ac.jp/aedir/

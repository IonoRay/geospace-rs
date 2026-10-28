# Official OMNI reference extract

`hro-may2024-reference.asc` contains three verbatim records from NASA SPDF:

https://spdf.gsfc.nasa.gov/pub/data/omni/high_res_omni/monthly_1min/omni_min202405.asc

Retrieved 2026-10-28. SHA-256 of the complete upstream ASCII file:
`b5aaa14f4daf0b03d697fa4380f659ba2a4bf8e1df223b320df087b2772acca5`.
Selected upstream line numbers: 12961, 14041, 14501 (one-based).
Epochs: 2024-05-10 00:00, 2024-05-10 18:00, 2024-05-11 01:40 UTC.

These are HRO records, not HRO2. Column definitions:
https://omniweb.gsfc.nasa.gov/html/HROdocum.html
Tests compare HRO CDF scalar values against the published ASCII precision,
including a missing-data record and negative magnetic components. They do not
derive expected values from the Rust reader. Source spacecraft, data teams,
NASA SPDF, and the OMNI project retain data attribution; this extract is not
relicensed as original IonoRay software.

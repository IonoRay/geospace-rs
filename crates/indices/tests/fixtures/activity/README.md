# Real activity-index fixtures

These are small verbatim extracts from official GFZ Hpo v3 files and DTU PCN
files. Exact upstream URLs, full-source SHA-256 hashes, and one-based original
line numbers are recorded in `provenance.json`. Headers retain the providers,
CC BY 4.0 notices, units, native timestamps, missing markers and citation text.
Integration/reference checks were performed on 2026-10-29.

- `hp30.txt`, `hp60.txt`: source header plus selected 2024-05-10 records,
  including observed Hp values above 9; no upper clipping is appropriate.
- `hp30-api.json`: independent GFZ API response for
  `https://kp.gfz.de/app/json/?start=2024-05-10T00:00:00Z&end=2024-05-11T00:00:00Z&index=Hp30`.
  The API includes the end timestamp; source reads use `[start,end)`.
- `pcn-iaga.txt`: definitive PCN, first hour and 09:00 hour on 2024-05-10,
  plus a genuine missing record on 2024-01-02. Negative PC values remain valid;
  `99999.00` is the documented missing sentinel.
- `pcn-provisional.csv`: first four source rows from DTU's 2026-01-01
  provisional file, retaining the observed values and cadence.
- `pcn-quicklook.csv`: first four source rows from the officially listed
  2026-10-28 quicklook file, preserving that edition rather than promoting it.
- `pcn-hourly-reference.json`: two data points extracted from the official ISGI
  public hourly plot, series `Definitive PCN`. The exact POST form parameters
  are retained in that JSON. Timestamps are hour midpoints. Tests aggregate
  60 native DTU minute values only for this independent comparison; production
  output is never resampled. The original HTML is not bundled.

Source references: https://kp.gfz.de/hp30-hp60/daten and
https://www.space.dtu.dk/english/research-divisions/geomagnetism-and-geospace/research-areas/Ground-based-magnetometry-instrumentration-infrastructure/PCN-index .
The data retain their own attribution/license; they are not relicensed as
original IonoRay software. Synthetic malformed-input, truncation and metadata
revision tests are explicitly marked in code and are not scientific samples.

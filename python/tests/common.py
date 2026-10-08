"""Shared fixed input from S2/S3's pinned official snapshot; no live user home."""
from functools import lru_cache
import tempfile
import ionoray_geospace as gs

P = dict(at="2020-07-01T12:00:00Z", latitude_deg=30., longitude_deg=120., altitude_km=300.)
IRI = dict(rz12=5.940666666666667, ig12=-5.526666666666667, f107_daily=71.2, f107_81_day=72.1)
HISTORY = dict(zip(("daily", "current", "three_hours_ago", "six_hours_ago", "nine_hours_ago",
                    "average_12_to_33_hours", "average_36_to_57_hours"), (4., 2., 4., 3., 4., 3.375, 2.5)))
MSIS = dict(f107a=69.9543209876543, f107_previous_day=68.1, ap_history=HISTORY)

@lru_cache(maxsize=1)
def baselines():
    with tempfile.TemporaryDirectory(prefix="s4-baselines-") as home:
        with gs.Session(home=home, data_policy="offline") as session:
            return {m: getattr(session, f"prepare_{m}")(**P) for m in ("iri", "hwm", "msis")}

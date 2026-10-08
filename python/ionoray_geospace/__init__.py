"""Python entry points for IonoRay's explicit models and driver sessions."""

import atexit
import weakref

from ._native import (
    capabilities, init_tracing as _init_tracing, igrf, iri, hwm, msis,
    Session, GeospaceError,
)

_tracing_guards: weakref.WeakSet[object] = weakref.WeakSet()


def init_tracing():
    """Install tracing and register its guard for normal interpreter shutdown."""
    guard = _init_tracing()
    _tracing_guards.add(guard)
    return guard


def _close_tracing_guards() -> None:
    for guard in list(_tracing_guards):
        try:
            guard.close()
        except RuntimeError:
            # Active calls cannot be joined safely at arbitrary interpreter exit.
            pass


atexit.register(_close_tracing_guards)

__all__ = ["capabilities", "init_tracing", "igrf", "iri", "hwm", "msis",
           "Session", "GeospaceError"]

from . import _native
for _capability, _name in [("iri", "PreparedIri"), ("hwm", "PreparedHwm"), ("msis", "PreparedMsis")]:
    if _capability in capabilities():
        globals()[_name] = getattr(_native, _name)
        __all__.append(_name)
del _capability, _name

from .types import (DataPolicy, Activity, ApHistory, IgrfResult, IriResult, HwmResult,
                    MsisResult, IriEvaluation, HwmEvaluation, MsisEvaluation)

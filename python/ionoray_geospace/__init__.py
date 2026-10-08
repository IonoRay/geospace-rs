"""Python entry points for IonoRay's explicit models and driver sessions."""

import atexit
import weakref

from ._native import (
    capabilities, init_tracing as _init_tracing, igrf, iri, hwm, msis,
    Session, PreparedIri, PreparedHwm, PreparedMsis, GeospaceError,
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
           "Session", "PreparedIri", "PreparedHwm", "PreparedMsis", "GeospaceError"]

from .types import (DataPolicy, Activity, ApHistory, IgrfResult, IriResult, HwmResult,
                    MsisResult, IriEvaluation, HwmEvaluation, MsisEvaluation)

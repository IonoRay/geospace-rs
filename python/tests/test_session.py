"""Session thread ownership, close semantics, GIL release, process exit."""
import gc
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import unittest
import ionoray_geospace as gs
from .common import P, IRI, MSIS, baselines


class SessionTests(unittest.TestCase):
    def test_repeated_calls_explicit_drivers_and_policy(self):
        with tempfile.TemporaryDirectory() as home, gs.Session(home=Path(home), data_policy="offline") as session:
            for model, drivers in [("iri", IRI), ("hwm", dict(activity="quiet")), ("msis", MSIS)]:
                first = getattr(session, f"evaluate_{model}")(**P, **drivers)
                for policy in [None, "ensure", "offline", "refresh"]:
                    self.assertEqual(first, getattr(session, f"evaluate_{model}")(**P, **drivers, data_policy=policy))
            self.assertEqual(list((Path(home)/"indices").iterdir()), [])
            with self.assertRaises(ValueError): session.prepare_iri(**P, data_policy="bad")
            with self.assertRaises(TypeError): session.prepare_iri(**P, data_policy=True)

    def test_auto_and_partial_overrides(self):
        with tempfile.TemporaryDirectory() as home, gs.Session(home=home, data_policy="offline") as session:
            for model in ["iri", "hwm", "msis"]:
                self.assertEqual(getattr(session, f"evaluate_{model}")(**P), baselines()[model].evaluate())
            partial = session.prepare_iri(**P, ig12=-5, rz12=0)
            self.assertIsNone(partial.indices["ig12"])
            self.assertIsNotNone(partial.indices["f107_daily"])

    def test_close_and_context_do_not_swallow_exception(self):
        with tempfile.TemporaryDirectory() as home:
            session = gs.Session(home=home, data_policy="offline")
            prepared = session.prepare_hwm(**P, activity="quiet")
            session.close(); session.close()
            for call in [lambda: session.prepare_hwm(**P), lambda: session.evaluate_iri(**P), session.__enter__]:
                with self.assertRaises(RuntimeError): call()
            del session; gc.collect()
            self.assertEqual(prepared.evaluate()["result"], gs.hwm(**P, activity="quiet"))
            with self.assertRaisesRegex(LookupError, "user failure"):
                with gs.Session(home=home, data_policy="offline") as session:
                    raise LookupError("user failure")
            with self.assertRaises(RuntimeError): session.prepare_hwm(**P)

    def test_wrong_thread_raises_runtime_error(self):
        with tempfile.TemporaryDirectory() as home, gs.Session(home=home, data_policy="offline") as session:
            errors = []
            def worker():
                for call in [lambda: session.prepare_hwm(**P), lambda: session.evaluate_iri(**P), session.close, session.__enter__]:
                    try: call()
                    except Exception as error: errors.append(error)
            thread = threading.Thread(target=worker); thread.start(); thread.join(10)
            self.assertFalse(thread.is_alive())
            self.assertEqual(len(errors), 4)
            self.assertTrue(all(isinstance(e, RuntimeError) for e in errors))
            self.assertEqual(session.evaluate_hwm(**P, activity="quiet")["result"], gs.hwm(**P, activity="quiet"))

    def test_data_errors_have_codes_and_constructor_fails(self):
        with tempfile.TemporaryDirectory() as home:
            file = Path(home)/"file"; file.write_text("not a directory")
            with self.assertRaises(gs.GeospaceError) as caught: gs.Session(home=file, data_policy="offline")
            self.assertEqual(caught.exception.code, "data_access")
            self.assertEqual(caught.exception.message, str(caught.exception))
            with gs.Session(home=home, data_policy="offline") as session:
                with self.assertRaises(gs.GeospaceError) as caught:
                    session.prepare_msis(**(P | {"at": "1900-01-01T00:00:00Z"}))
                self.assertEqual(caught.exception.code, "data_unavailable")

    def test_gil_released_inside_real_calls(self):
        # A long switch interval prevents a Python bytecode timeslice from passing
        # this test. The observer must run between entering and returning from native.
        old = sys.getswitchinterval()
        try:
            sys.setswitchinterval(100.)
            with tempfile.TemporaryDirectory() as home, gs.Session(home=home, data_policy="offline") as session:
                prepared = session.prepare_iri(**P, **IRI)
                calls = [lambda: gs.igrf(**P), lambda: gs.iri(**P, **IRI),
                         lambda: gs.hwm(**P, activity="quiet"), lambda: gs.msis(**P, **MSIS),
                         prepared.evaluate, lambda: session.prepare_hwm(**P)]
                for call in calls:
                    ready = threading.Event(); go = threading.Event(); progressed = []
                    def observer():
                        ready.set(); go.wait(); progressed.append(True)
                    thread = threading.Thread(target=observer); thread.start(); ready.wait(5)
                    go.set()
                    # Very short native calls can finish before the observer is
                    # scheduled. Retry bounded work without a Python timeslice.
                    for _ in range(100):
                        call()
                        if progressed: break
                    during_call = bool(progressed)
                    thread.join(5)
                    self.assertTrue(during_call, repr(call))
        finally: sys.setswitchinterval(old)

    def test_normal_exit_and_foreign_thread_destructor(self):
        with tempfile.TemporaryDirectory() as home:
            source = f'''import gc, threading, ionoray_geospace as gs
p = {P!r}
s = gs.Session(home={home!r}, data_policy="offline")
b = s.prepare_hwm(**p, activity="quiet")
s.close(); s.close()
s = gs.Session(home={home!r}, data_policy="offline")
box = [s]; del s
t = threading.Thread(target=lambda: (box.clear(), gc.collect()))
t.start(); t.join()
assert b.evaluate()["result"] == gs.hwm(**p, activity="quiet")
retained = gs.Session(home={home!r}, data_policy="offline")
print("exit-ok")
'''
            result = subprocess.run([sys.executable, "-c", source], capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), "exit-ok")
            self.assertNotIn("Exception ignored", result.stderr)


if __name__ == "__main__": unittest.main()

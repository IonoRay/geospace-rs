"""Actual wheel contracts that also work with no model compiled."""
from pathlib import Path
import tempfile
import unittest
import ionoray_geospace as gs

class CapabilityTests(unittest.TestCase):
    def test_functions_and_prepared_classes_follow_actual_features(self):
        caps = gs.capabilities()
        self.assertIn('indices', caps)
        for name in ['igrf', 'iri', 'hwm', 'msis']:
            if name not in caps:
                with self.assertRaises(gs.GeospaceError) as caught:
                    getattr(gs, name)()
                self.assertEqual(caught.exception.code, 'model_unavailable')
        for name, cls in [('iri', 'PreparedIri'), ('hwm', 'PreparedHwm'), ('msis', 'PreparedMsis')]:
            self.assertEqual(hasattr(gs, cls), name in caps)
            self.assertEqual(hasattr(gs.Session, 'prepare_'+name), name in caps)

    def test_base_session_lifecycle_and_explicit_store(self):
        with tempfile.TemporaryDirectory() as home:
            session = gs.Session(home=Path(home), data_policy='offline')
            with session:
                self.assertIsNotNone(session)
            session.close()
            with self.assertRaises(RuntimeError):
                session.__enter__()
            with self.assertRaisesRegex(LookupError, 'caller error'):
                with gs.Session(home=home, data_policy='offline'):
                    raise LookupError('caller error')

if __name__ == '__main__':
    unittest.main()

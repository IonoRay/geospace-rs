//! White-box resource identity test supplements Python lifecycle tests.
use super::Session;
use pyo3::{prelude::*, types::PyDict};

#[test]
fn repeated_calls_keep_one_store_and_runtime() {
    Python::attach(|py| {
        let home = std::env::temp_dir().join(format!("geospace-s4-session-{}", std::process::id()));
        let mut session = Session::new(py, Some(home.clone()), "offline").unwrap();
        let identity = |s: &Session| {
            let resources = s.resources().unwrap();
            (
                std::ptr::from_ref(&resources.geospace),
                std::ptr::from_ref(&resources.runtime),
            )
        };
        let original = identity(&session);
        let dict = py.eval(pyo3::ffi::c_str!("dict(at='2020-07-01T12:00:00Z', latitude_deg=30., longitude_deg=120., altitude_km=300., activity='quiet')"), None, None).unwrap().cast_into::<PyDict>().unwrap();
        let first = session.evaluate_hwm(py, Some(&dict)).unwrap();
        assert_eq!(original, identity(&session));
        let second = session.evaluate_hwm(py, Some(&dict)).unwrap();
        assert_eq!(original, identity(&session));
        assert!(first.bind(py).eq(second.bind(py)).unwrap());
        session.close(py).unwrap();
        session.close(py).unwrap();
        assert!(session.resources().is_err());
        std::fs::remove_dir_all(home).unwrap();
    });
}

//! Thin Python bindings for explicit models, automatic sessions and frozen inputs.
mod api;
#[cfg(any(feature = "igrf", feature = "iri", feature = "hwm", feature = "msis"))]
mod convert;
mod error;
mod native;
#[cfg(any(feature = "iri", feature = "hwm", feature = "msis"))]
mod prepared;
mod request;
mod session;
#[cfg(test)]
mod tests;
mod tracing;

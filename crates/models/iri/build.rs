//! Build-time acquisition and local compilation of the official IRI-2020 release.

mod build_fortran;
mod build_manifest;
mod build_support;

use std::env;

use build_manifest::{
    ASSET_SET_SHA256, GLOBAL_OFFLINE_ENV, OFFLINE_ENV, RELEASE_SHA256, SOURCE_ENV,
};

fn main() {
    println!("cargo:rerun-if-env-changed=IONORAY_CACHE_ROOT");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build_fortran.rs");
    println!("cargo:rerun-if-changed=build_manifest.rs");
    println!("cargo:rerun-if-changed=build_support.rs");
    println!("cargo:rerun-if-changed=IRI-LICENSE.txt");
    println!("cargo:rerun-if-env-changed={SOURCE_ENV}");
    println!("cargo:rerun-if-env-changed={OFFLINE_ENV}");
    println!("cargo:rerun-if-env-changed={GLOBAL_OFFLINE_ENV}");
    println!("cargo:rerun-if-env-changed=FC");
    println!("cargo:rerun-if-env-changed=AR");
    println!("cargo:rustc-env=IONORAY_IRI2020_RELEASE_SHA256={RELEASE_SHA256}");
    println!("cargo:rustc-env=IONORAY_IRI2020_ASSET_SET_SHA256={ASSET_SET_SHA256}");
    if env::var_os("CARGO_FEATURE_IRI2020").is_none() {
        return;
    }
    if let Err(error) = build_support::build_backend() {
        panic!("cannot prepare the IRI-2020 backend: {error}");
    }
}

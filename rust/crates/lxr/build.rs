//! Build script of `pgr`. It does nothing unless the `embedded-model` feature is on.
//!
//! With the feature, the program carries the embedding model inside it, read from the folder
//! that the environment variable `PGR_EMBED_MODEL_DIR` names. This script checks that the folder
//! is there before the compiler starts, so that a mistake is one clear message. When the variable
//! is not set it sets the cfg `pgr_embed_model_missing`, and the source turns that into a compile
//! error that says what to do.

use std::env;
use std::path::PathBuf;

/// The files of the model that the program reads.
const MODEL_FILES: [&str; 2] = ["model.safetensors", "tokenizer.json"];

fn main() {
    println!("cargo:rerun-if-env-changed=PGR_EMBED_MODEL_DIR");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-check-cfg=cfg(pgr_embed_model_missing)");
    if env::var_os("CARGO_FEATURE_EMBEDDED_MODEL").is_none() {
        return;
    }
    let Some(dir) = env::var_os("PGR_EMBED_MODEL_DIR").filter(|value| !value.is_empty()).map(PathBuf::from) else {
        println!("cargo:rustc-cfg=pgr_embed_model_missing");
        return;
    };
    // A build script runs in the folder of its package, not where the person ran cargo, so a
    // relative path would mean another folder here than it meant to the person.
    assert!(dir.is_absolute(), "PGR_EMBED_MODEL_DIR must be an absolute path, not {}", dir.display());
    for name in MODEL_FILES {
        let file = dir.join(name);
        assert!(file.is_file(), "PGR_EMBED_MODEL_DIR is {} but it has no {name}", dir.display());
        println!("cargo:rerun-if-changed={}", file.display());
    }
}

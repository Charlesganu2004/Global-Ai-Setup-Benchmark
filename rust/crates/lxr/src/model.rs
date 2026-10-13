//! Finding and opening the embedding model.
//!
//! The order of search is fixed: the folder named by `LXR_MODEL_DIR`, then the model that is
//! compiled into the program (the `embedded-model` feature), then the places
//! `lxr_text::Embedder::locate` looks: the folder a pip wheel installs next to the program,
//! `~/.local-index/models`, and the Hugging Face cache. The first place that has a model is
//! the one used; a model that is there but cannot be opened means no embeddings, as it
//! always did, and the later places are not tried.

use lxr_text::Embedder;
use std::path::PathBuf;

/// The model compiled into the program. Without the `embedded-model` feature there is none and
/// nothing about the program changes.
#[cfg(all(feature = "embedded-model", not(pgr_embed_model_missing)))]
mod embedded {
    use lxr_text::Embedder;

    /// `tokenizer.json`, from the folder `PGR_EMBED_MODEL_DIR` named when the program was built.
    const TOKENIZER: &[u8] = include_bytes!(concat!(env!("PGR_EMBED_MODEL_DIR"), "/tokenizer.json"));

    /// `model.safetensors`, from the same folder.
    const WEIGHTS: &[u8] = include_bytes!(concat!(env!("PGR_EMBED_MODEL_DIR"), "/model.safetensors"));

    /// True: this program carries a model.
    pub fn present() -> bool {
        true
    }

    /// Opens the model out of the program's own bytes. Nothing is copied.
    pub fn open() -> Option<Embedder> {
        Embedder::from_bytes(TOKENIZER, WEIGHTS).ok()
    }
}

// The feature is on and the variable is not set: the build stops here with a message
// (`build.rs` sets the cfg).
#[cfg(all(feature = "embedded-model", pgr_embed_model_missing))]
compile_error!(
    "the embedded-model feature needs the environment variable PGR_EMBED_MODEL_DIR at build time: \
     set it to the absolute path of the folder that holds model.safetensors and tokenizer.json"
);

/// No model is compiled in. (With the feature on and the variable unset this stands in for the
/// module above, so that the message above is the only error.)
#[cfg(any(not(feature = "embedded-model"), pgr_embed_model_missing))]
mod embedded {
    use lxr_text::Embedder;

    pub fn present() -> bool {
        false
    }

    pub fn open() -> Option<Embedder> {
        None
    }
}
/// Where the model is taken from.
#[derive(Debug, PartialEq, Eq)]
pub enum Source {
    /// A folder with `model.safetensors` and `tokenizer.json`.
    Folder(PathBuf),
    /// The model compiled into the program.
    Embedded,
}

/// The order of search. `env` is the folder `LXR_MODEL_DIR` names when it holds the model,
/// `embedded` says whether the program carries a model, and `installed` finds the other places
/// and is called only when the first two have nothing.
pub fn choose(env: Option<PathBuf>, embedded: bool, installed: impl FnOnce() -> Option<PathBuf>) -> Option<Source> {
    if let Some(dir) = env {
        return Some(Source::Folder(dir));
    }
    if embedded {
        return Some(Source::Embedded);
    }
    installed().map(Source::Folder)
}

/// The place the model is taken from on this run, if there is one.
pub fn source() -> Option<Source> {
    choose(Embedder::locate_env(), embedded::present(), Embedder::locate)
}

/// True when this run has a model to use, whether or not it can be opened.
pub fn is_present() -> bool {
    source().is_some()
}

/// Finds and opens the embedding model. `None` when there is none or it cannot be opened.
pub fn open() -> Option<Embedder> {
    match source()? {
        Source::Folder(dir) => Embedder::open(&dir).ok(),
        Source::Embedded => embedded::open(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> Option<PathBuf> {
        Some(PathBuf::from(name))
    }

    #[test]
    fn lxr_model_dir_comes_first_whether_or_not_a_model_is_compiled_in() {
        for embedded in [false, true] {
            let chosen = choose(dir("named"), embedded, || panic!("the other places are not looked at"));
            assert_eq!(chosen, Some(Source::Folder(PathBuf::from("named"))));
        }
    }

    #[test]
    fn the_compiled_in_model_comes_before_the_places_it_looks_today() {
        let chosen = choose(None, true, || panic!("the other places are not looked at"));
        assert_eq!(chosen, Some(Source::Embedded));
    }

    #[test]
    fn without_a_compiled_in_model_the_places_it_looks_today_are_used() {
        assert_eq!(choose(None, false, || dir("installed")), Some(Source::Folder(PathBuf::from("installed"))));
        assert_eq!(choose(None, false, || None), None);
    }

    #[test]
    fn this_build_knows_whether_it_carries_a_model() {
        assert_eq!(embedded::present(), cfg!(feature = "embedded-model"));
    }
}

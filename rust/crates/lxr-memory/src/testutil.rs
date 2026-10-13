//! Helpers shared by the tests: temporary folders, a fake view of the code, the embedder.

use crate::{Anchor, CodeView, RecallOpts, RetainOpts};
use lxr_text::Embedder;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;

/// Wednesday 2026-03-11, 12:00 UTC: the clock of most tests.
pub(crate) const NOW: i64 = 1_773_230_400;
/// Seconds in a day.
pub(crate) const DAY: i64 = 86_400;

/// A folder that is removed when it goes out of scope.
pub(crate) struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> TempDir {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("lxr-memory-{tag}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create the test folder");
        TempDir(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// A folder inside it for a bank.
    pub fn bank(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The embedder. Without a model the tests that need one cannot run, and a green run that
/// skipped them would say nothing about them, so a missing model fails the test. Setting
/// `LXR_SKIP_MODEL_TESTS` skips those tests on purpose: this then returns `None`.
pub(crate) fn embedder() -> Option<&'static Embedder> {
    static EMBEDDER: OnceLock<Option<Embedder>> = OnceLock::new();
    let found = EMBEDDER.get_or_init(|| Embedder::locate().and_then(|dir| Embedder::open(&dir).ok())).as_ref();
    if found.is_none() {
        assert!(
            std::env::var_os("LXR_SKIP_MODEL_TESTS").is_some(),
            "no embedding model is installed, so the tests that need one cannot run; install it, or set LXR_SKIP_MODEL_TESTS=1 to skip them"
        );
        eprintln!("skipped: no embedding model is installed");
    }
    found
}

/// Retain options with a fixed clock.
pub(crate) fn retain_at(now: i64) -> RetainOpts {
    RetainOpts { now: Some(now), ..RetainOpts::default() }
}

/// Recall options with a fixed clock.
pub(crate) fn recall_at(now: i64) -> RecallOpts {
    RecallOpts { now: Some(now), ..RecallOpts::default() }
}

/// A small fake of the code: names that map to a file and a hash, which tests can change.
#[derive(Default)]
pub(crate) struct FakeCode {
    defs: RefCell<HashMap<String, (String, u64)>>,
}

impl FakeCode {
    pub fn define(&self, name: &str, path: &str, hash: u64) {
        self.defs.borrow_mut().insert(name.to_string(), (path.to_string(), hash));
    }

    pub fn remove(&self, name: &str) {
        self.defs.borrow_mut().remove(name);
    }
}

impl CodeView for FakeCode {
    fn anchor(&self, word: &str) -> Option<Anchor> {
        self.defs.borrow().get(word).map(|(path, hash)| Anchor { path: path.clone(), qual: word.to_string(), hash: *hash })
    }

    fn current_hash(&self, anchor: &Anchor) -> Option<u64> {
        self.defs.borrow().get(&anchor.qual).map(|(_, hash)| *hash)
    }
}

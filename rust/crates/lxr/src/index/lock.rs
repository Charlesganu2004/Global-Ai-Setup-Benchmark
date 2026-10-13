//! The refresh lock: one process writes the index at a time.

use super::LOCK_FILE;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};

/// A lock file older than this many seconds is ignored: its owner is gone.
const STALE_AFTER_SECS: u64 = 600;
/// How long a refresh waits for another one before it gives up.
const WAIT_LIMIT: Duration = Duration::from_secs(30);
/// How often the lock is looked at while waiting.
const POLL: Duration = Duration::from_millis(40);

/// The held lock. The file is removed when this is dropped.
pub struct Lock {
    path: PathBuf,
}

impl Lock {
    /// Takes the lock for the index in `dir`, waiting for another process that holds it.
    ///
    /// `Ok(None)` when the wait ran out, so the caller can go on with the index as it is.
    /// A lock file older than 600 seconds is removed and taken over.
    pub fn acquire(dir: &Path) -> std::io::Result<Option<Lock>> {
        fs::create_dir_all(dir)?;
        let path = dir.join(LOCK_FILE);
        let started = Instant::now();
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    let _ = writeln!(file, "{}", std::process::id());
                    return Ok(Some(Lock { path }));
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                    if is_stale(&path) {
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    if started.elapsed() >= WAIT_LIMIT {
                        return Ok(None);
                    }
                    sleep(POLL);
                }
                Err(error) => return Err(error),
            }
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// True when the lock file was last written more than 600 seconds ago.
fn is_stale(path: &Path) -> bool {
    let modified = match fs::metadata(path).and_then(|m| m.modified()) {
        Ok(time) => time,
        // The file vanished: the next attempt will find the lock free.
        Err(_) => return false,
    };
    SystemTime::now().duration_since(modified).map(|age| age.as_secs() > STALE_AFTER_SECS).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_lock_waits_until_the_first_is_dropped_or_stale() {
        let dir = std::env::temp_dir().join(format!("pgr-lock-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let first = Lock::acquire(&dir).unwrap().expect("free lock");
        assert!(dir.join(LOCK_FILE).exists());
        drop(first);
        assert!(!dir.join(LOCK_FILE).exists());
        let again = Lock::acquire(&dir).unwrap();
        assert!(again.is_some());
        drop(again);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_fresh_lock_file_is_not_stale() {
        let dir = std::env::temp_dir().join(format!("pgr-lock-fresh-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join(LOCK_FILE);
        fs::write(&file, "1\n").unwrap();
        assert!(!is_stale(&file));
        let _ = fs::remove_dir_all(&dir);
    }
}

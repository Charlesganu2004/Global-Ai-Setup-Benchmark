//! The refresh lock: one process writes the index at a time.
//!
//! The lock is the file `refresh.lock`, made whole by creating it where none exists, and it
//! holds a token that belongs to its owner. A file older than 600 seconds belongs to a
//! process that is gone and may be taken over. Taking it over, and giving the lock back, is
//! done under an operating system lock on `refresh.guard`, which the system releases when a
//! process dies. Inside it the age is looked at again, so two processes that saw the same
//! expired file cannot both remove it, and one that saw it long ago cannot remove the file of
//! the owner who took over meanwhile. The owner removes the file only while the token in it
//! is still its own.

use super::{GUARD_FILE, LOCK_FILE};
use crate::oslock::OsLock;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A lock file older than this is ignored: its owner is gone.
const STALE_AFTER: Duration = Duration::from_secs(600);
/// How long a refresh waits for another one before it gives up.
const WAIT_LIMIT: Duration = Duration::from_secs(30);
/// How often the lock is looked at while waiting.
const POLL: Duration = Duration::from_millis(40);

/// How long to wait for the lock: 30 seconds, or the number of milliseconds in
/// `PGR_LOCK_WAIT_MS` when that holds one.
fn wait_limit() -> Duration {
    wait_limit_from(std::env::var("PGR_LOCK_WAIT_MS").ok().as_deref())
}

fn wait_limit_from(setting: Option<&str>) -> Duration {
    setting.and_then(|text| text.trim().parse::<u64>().ok()).map_or(WAIT_LIMIT, Duration::from_millis)
}

/// A token no other lock has: the process, the time and a count.
fn new_token() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    format!("{}-{}-{}\n", std::process::id(), nanos, NEXT.fetch_add(1, Ordering::Relaxed))
}

/// The held lock. The file is removed when this is dropped, if it is still this lock's.
pub struct Lock {
    path: PathBuf,
    guard: PathBuf,
    token: String,
}

impl Lock {
    /// Takes the lock for the index in `dir`, waiting for another process that holds it.
    ///
    /// `Ok(None)` when the wait ran out. A lock file older than 600 seconds is taken over.
    pub fn acquire(dir: &Path) -> std::io::Result<Option<Lock>> {
        Lock::acquire_with(dir, wait_limit(), STALE_AFTER, &|| {})
    }

    /// Takes the lock when it is free right now, and does not wait for it otherwise.
    pub fn try_acquire(dir: &Path) -> std::io::Result<Option<Lock>> {
        Lock::acquire_with(dir, Duration::ZERO, STALE_AFTER, &|| {})
    }

    /// `acquire` with its wait and its expiry, and a call that runs once a lock file was
    /// found expired, before it is taken over (a test holds a process here).
    fn acquire_with(dir: &Path, wait: Duration, stale_after: Duration, observed: &dyn Fn()) -> std::io::Result<Option<Lock>> {
        fs::create_dir_all(dir)?;
        let path = dir.join(LOCK_FILE);
        let guard = dir.join(GUARD_FILE);
        let token = new_token();
        let started = Instant::now();
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    if let Err(error) = file.write_all(token.as_bytes()) {
                        drop(file);
                        let _ = fs::remove_file(&path);
                        return Err(error);
                    }
                    return Ok(Some(Lock { path, guard, token }));
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                    if is_stale_after(&path, stale_after) {
                        observed();
                        take_over(&path, &guard, stale_after);
                        if !path.exists() {
                            continue;
                        }
                    }
                }
                // On Windows a file that was deleted while another process still has it open
                // cannot be created again for a moment.
                Err(error) if error.kind() == ErrorKind::PermissionDenied && started.elapsed() < wait => {}
                Err(error) => return Err(error),
            }
            if started.elapsed() >= wait {
                return Ok(None);
            }
            sleep(POLL.min(wait.saturating_sub(started.elapsed())).max(Duration::from_millis(1)));
        }
    }

    /// True while the lock file still holds this lock's token, that is, while nobody took
    /// the lock over from this process.
    pub fn is_ours(&self) -> bool {
        fs::read_to_string(&self.path).map_or(false, |text| text == self.token)
    }

    /// Marks the lock as still in use, so that a long build is not taken for a dead one.
    /// False when the lock is no longer this one's.
    pub fn touch(&self) -> bool {
        let _order = OsLock::take(&self.guard).ok();
        if !self.is_ours() {
            return false;
        }
        OpenOptions::new().write(true).open(&self.path).and_then(|file| file.set_modified(SystemTime::now())).is_ok()
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _order = OsLock::take(&self.guard).ok();
        if self.is_ours() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Removes the lock file if it is still expired once the guard is held. A file system that
/// cannot lock leaves the take-over unordered.
fn take_over(path: &Path, guard: &Path, stale_after: Duration) {
    let _order = OsLock::take(guard).ok();
    if is_stale_after(path, stale_after) {
        let _ = fs::remove_file(path);
    }
}

/// True when the lock file was last written more than `stale_after` ago.
fn is_stale_after(path: &Path, stale_after: Duration) -> bool {
    let modified = match fs::metadata(path).and_then(|m| m.modified()) {
        Ok(time) => time,
        // The file vanished: the next attempt will find the lock free.
        Err(_) => return false,
    };
    SystemTime::now().duration_since(modified).map(|age| age > stale_after).unwrap_or(false)
}

/// True when the lock file was last written more than 600 seconds ago.
#[cfg(test)]
fn is_stale(path: &Path) -> bool {
    is_stale_after(path, STALE_AFTER)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pgr-lock-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn set_time(path: &Path, time: SystemTime) {
        let file = OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(time).unwrap();
    }

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
    fn a_reclaim_that_was_overtaken_does_not_remove_the_lock_of_the_new_owner() {
        use std::sync::mpsc;
        let dir = scratch("race");
        let file = dir.join(LOCK_FILE);
        fs::write(&file, "999999\n").unwrap();
        set_time(&file, SystemTime::now() - Duration::from_secs(700));

        let (seen_tx, seen_rx) = mpsc::channel::<()>();
        let (go_tx, go_rx) = mpsc::channel::<()>();
        let racer = {
            let dir = dir.clone();
            std::thread::spawn(move || {
                Lock::acquire_with(&dir, Duration::from_millis(300), STALE_AFTER, &|| {
                    seen_tx.send(()).unwrap();
                    go_rx.recv().unwrap();
                })
            })
        };
        // The racer has seen the expired lock and waits just before it would remove it.
        seen_rx.recv().unwrap();
        let owner = Lock::acquire(&dir).unwrap().expect("the expired lock is taken over");
        go_tx.send(()).unwrap();
        let racer = racer.join().unwrap().unwrap();
        assert!(racer.is_none(), "a lock that a live owner holds must not be taken");
        assert!(file.exists(), "the new owner's lock file must still be there");
        assert!(owner.is_ours());
        drop(owner);
        assert!(!file.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn many_threads_that_see_one_expired_lock_take_it_one_at_a_time() {
        use std::sync::atomic::AtomicUsize;
        use std::sync::{Arc, Barrier};
        let dir = scratch("many");
        let file = dir.join(LOCK_FILE);
        let holders = Arc::new(AtomicUsize::new(0));
        let taken = Arc::new(AtomicUsize::new(0));
        for _round in 0..20 {
            fs::write(&file, "999999\n").unwrap();
            set_time(&file, SystemTime::now() - Duration::from_secs(700));
            let barrier = Arc::new(Barrier::new(4));
            let threads: Vec<_> = (0..4)
                .map(|_| {
                    let (dir, barrier, holders, taken) = (dir.clone(), barrier.clone(), holders.clone(), taken.clone());
                    std::thread::spawn(move || {
                        barrier.wait();
                        let lock = Lock::acquire_with(&dir, Duration::from_secs(20), STALE_AFTER, &|| {}).unwrap().expect("a lock in time");
                        let now = holders.fetch_add(1, Ordering::SeqCst) + 1;
                        assert_eq!(now, 1, "two threads hold the lock at once");
                        sleep(Duration::from_millis(3));
                        holders.fetch_sub(1, Ordering::SeqCst);
                        taken.fetch_add(1, Ordering::SeqCst);
                        drop(lock);
                    })
                })
                .collect();
            for thread in threads {
                thread.join().unwrap();
            }
        }
        assert_eq!(taken.load(Ordering::SeqCst), 80);
        assert!(!file.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_lock_that_was_taken_over_is_not_removed_by_its_old_owner() {
        let dir = scratch("successor");
        let old = Lock::acquire(&dir).unwrap().expect("free lock");
        // The file now belongs to somebody else, as after a take-over of an expired lock.
        fs::write(dir.join(LOCK_FILE), "someone-else\n").unwrap();
        assert!(!old.is_ours());
        assert!(!old.touch());
        drop(old);
        assert_eq!(fs::read_to_string(dir.join(LOCK_FILE)).unwrap(), "someone-else\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn touching_the_lock_keeps_it_from_expiring() {
        let dir = scratch("touch");
        let lock = Lock::acquire(&dir).unwrap().expect("free lock");
        let file = dir.join(LOCK_FILE);
        set_time(&file, SystemTime::now() - Duration::from_secs(700));
        assert!(is_stale(&file));
        assert!(lock.touch());
        assert!(!is_stale(&file));
        drop(lock);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_lock_that_is_held_is_given_up_on_after_the_wait() {
        let dir = scratch("wait");
        let held = Lock::acquire(&dir).unwrap().expect("free lock");
        let started = Instant::now();
        let second = Lock::acquire_with(&dir, Duration::from_millis(200), STALE_AFTER, &|| {}).unwrap();
        assert!(second.is_none());
        assert!(started.elapsed() >= Duration::from_millis(200));
        assert!(Lock::try_acquire(&dir).unwrap().is_none());
        drop(held);
        assert!(Lock::try_acquire(&dir).unwrap().is_some());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_wait_comes_from_the_setting_or_defaults_to_thirty_seconds() {
        assert_eq!(wait_limit_from(None), Duration::from_secs(30));
        assert_eq!(wait_limit_from(Some("250")), Duration::from_millis(250));
        assert_eq!(wait_limit_from(Some(" 0 ")), Duration::ZERO);
        assert_eq!(wait_limit_from(Some("soon")), Duration::from_secs(30));
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

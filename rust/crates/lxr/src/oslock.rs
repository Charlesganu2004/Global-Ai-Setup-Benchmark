//! An exclusive lock on a file that the operating system holds for the process. It is
//! released when the process ends, however it ends, so it can never be left behind.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;

/// The lock on one file. It is released when this is dropped.
pub struct OsLock(File);

impl OsLock {
    /// Waits for the lock on the file at `path`, which is created (with its folder) when
    /// it is missing. The file stays when the lock is released: it holds nothing.
    pub fn take(path: &Path) -> io::Result<OsLock> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new().create(true).write(true).truncate(false).open(path)?;
        file.lock()?;
        Ok(OsLock(file))
    }
}

impl Drop for OsLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn two_holders_are_never_inside_at_once() {
        let dir = std::env::temp_dir().join(format!("pgr-oslock-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("x.lock");
        let inside = Arc::new(AtomicUsize::new(0));
        let most = Arc::new(AtomicUsize::new(0));
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let (path, inside, most) = (path.clone(), inside.clone(), most.clone());
                std::thread::spawn(move || {
                    for _ in 0..10 {
                        let lock = OsLock::take(&path).unwrap();
                        let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                        most.fetch_max(now, Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(1));
                        inside.fetch_sub(1, Ordering::SeqCst);
                        drop(lock);
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(most.load(Ordering::SeqCst), 1);
        assert!(path.is_file(), "the lock file stays and holds nothing");
        let _ = fs::remove_dir_all(&dir);
    }
}

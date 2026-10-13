//! Helpers shared by the integration tests: a temporary workspace with its own index
//! home, a small Python project, and a runner for the built `pgr` binary.
//!
//! Every run points `LXR_HOME` at a folder inside the workspace, so the user's real
//! index is never touched, and hides any embedding model, so that answers are made of
//! words only and do not depend on the machine.
#![allow(dead_code)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

/// What one run of the binary gave.
pub struct Run {
    pub code: i32,
    pub out: String,
    pub err: String,
}

/// A temporary workspace: an index home, a fake user home and a project folder.
pub struct Env {
    pub base: PathBuf,
    pub home: PathBuf,
    pub user: PathBuf,
    pub proj: PathBuf,
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

pub const CORE_PY: &str = r#""""Core helpers."""


def load_config(path):
    """Read the configuration file at path."""
    return parse_text(read_text(path))


def read_text(path):
    """Return the text of a file."""
    with open(path) as handle:
        return handle.read()


def parse_text(text):
    """Turn text into a dictionary of settings."""
    result = {}
    for line in text.splitlines():
        key, _, value = line.partition("=")
        result[key.strip()] = value.strip()
    return result


class Store:
    """Keeps settings in memory."""

    def __init__(self, data):
        self.data = data

    def get(self, key):
        """Look up one setting."""
        return self.data.get(key)

    def reload(self, path):
        """Replace the settings with those in a file."""
        self.data = load_config(path)
        return self.get("name")
"#;

pub const CLI_PY: &str = r#""""Command line entry points."""

from app.core import Store, load_config


def main(argv):
    """Run the program."""
    store = Store(load_config(argv[0]))
    return report(store)


def report(store):
    """Print the settings that matter."""
    return store.get("name")


STARTUP = main(["settings.ini"])
"#;

pub const UTIL_PY: &str = r#"def slugify(text):
    """Make a url friendly name."""
    return text.lower().replace(" ", "-")


def unused_helper():
    """Nothing calls this."""
    return None
"#;

impl Env {
    /// An empty workspace.
    pub fn new() -> Env {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let base = std::env::temp_dir().join(format!("pgr-it-{}-{}", std::process::id(), n));
        let _ = fs::remove_dir_all(&base);
        let env = Env { home: base.join("home"), user: base.join("user"), proj: base.join("proj"), base };
        for dir in [&env.home, &env.user, &env.proj] {
            fs::create_dir_all(dir).unwrap();
        }
        env
    }

    /// A workspace holding the small Python project the tests ask about.
    pub fn with_project() -> Env {
        let env = Env::new();
        env.write("app/core.py", CORE_PY);
        env.write("app/cli.py", CLI_PY);
        env.write("app/util.py", UTIL_PY);
        env
    }

    /// Writes a file of the project.
    pub fn write(&self, rel: &str, text: &str) {
        let path = self.proj.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// Removes a file of the project.
    pub fn remove(&self, rel: &str) {
        fs::remove_file(self.proj.join(rel)).unwrap();
    }

    /// Reads a file of the project.
    pub fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.proj.join(rel)).unwrap()
    }

    /// The command with the environment of the tests.
    pub fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_pgr"));
        cmd.env("LXR_HOME", &self.home)
            .env("USERPROFILE", &self.user)
            .env("HOME", &self.user)
            .env("HF_HOME", self.user.join("hf"))
            .env("HF_HUB_CACHE", self.user.join("hf").join("hub"))
            .env_remove("LXR_MODEL_DIR")
            .env_remove("PGR_ENGINES")
            .env_remove("PGR_NO_REFRESH")
            .env_remove("PGR_BANK")
            .env_remove("PGR_TASK")
            .env_remove("PGR_ROLE");
        cmd
    }

    /// Runs `pgr --root PROJECT ARGS...`.
    pub fn run(&self, args: &[&str]) -> Run {
        self.run_with(args, &[])
    }

    /// Runs `pgr --root PROJECT ARGS...` with extra environment variables.
    pub fn run_with(&self, args: &[&str], vars: &[(&str, &str)]) -> Run {
        self.run_at(&self.proj, args, vars)
    }

    /// Runs `pgr --root ROOT ARGS...` with extra environment variables.
    pub fn run_at(&self, root: &Path, args: &[&str], vars: &[(&str, &str)]) -> Run {
        let mut cmd = self.command();
        cmd.arg("--root").arg(root).args(args);
        for (key, value) in vars {
            cmd.env(key, value);
        }
        finish(cmd.stdin(Stdio::null()).output().unwrap())
    }

    /// Runs `pgr ARGS...` inside `dir`, without `--root`.
    pub fn run_in(&self, dir: &Path, args: &[&str]) -> Run {
        let mut cmd = self.command();
        cmd.current_dir(dir).args(args);
        finish(cmd.stdin(Stdio::null()).output().unwrap())
    }

    /// Runs `pgr --root PROJECT ARGS...` with `input` on standard input.
    pub fn run_stdin(&self, args: &[&str], input: &str) -> Run {
        let mut cmd = self.command();
        cmd.arg("--root").arg(&self.proj).args(args);
        let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        finish(child.wait_with_output().unwrap())
    }

    /// The folder that holds the index of the project: the only folder in the index home.
    pub fn index_dir(&self) -> PathBuf {
        let mut dirs: Vec<PathBuf> = fs::read_dir(&self.home)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir() && p.file_name().map_or(false, |n| n.to_string_lossy().starts_with("proj-")))
            .collect();
        assert_eq!(dirs.len(), 1, "one index folder expected in {:?}", self.home);
        dirs.remove(0)
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

/// The offset of the machine's time zone from UTC in seconds, east positive, asked of the
/// operating system's own tools and not of `pgr`: `TimeZoneInfo` on Windows, `date` elsewhere.
/// `None` when they do not say.
pub fn machine_offset() -> Option<i64> {
    #[cfg(windows)]
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", "[int][TimeZoneInfo]::Local.GetUtcOffset([DateTime]::UtcNow).TotalSeconds"])
        .output()
        .ok()?;
    #[cfg(not(windows))]
    let output = Command::new("date").arg("+%z").output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    #[cfg(windows)]
    return text.parse().ok();
    #[cfg(not(windows))]
    {
        // `+0530`, `-0700`.
        let sign = if text.starts_with('-') { -1 } else { 1 };
        let digits = text.trim_start_matches(['+', '-']);
        let (hours, minutes) = (digits.get(..2)?.parse::<i64>().ok()?, digits.get(2..4)?.parse::<i64>().ok()?);
        Some(sign * (hours * 3600 + minutes * 60))
    }
}

/// A folder that cannot be listed for as long as the value lives.
pub struct Locked(PathBuf);

impl Locked {
    /// Takes the right to list `dir` away from everyone. `None` when this account can list the
    /// folder anyway (an administrator), so that the failure cannot be made.
    pub fn new(dir: &Path) -> Option<Locked> {
        #[cfg(windows)]
        {
            let _ = Command::new("icacls").arg(dir).args(["/deny", "*S-1-1-0:(RD)"]).output();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o000));
        }
        let locked = Locked(dir.to_path_buf());
        if fs::read_dir(dir).is_ok() {
            eprintln!("skipped: this account can list a folder it is denied");
            return None;
        }
        Some(locked)
    }
}

impl Drop for Locked {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            let _ = Command::new("icacls").arg(&self.0).args(["/remove:d", "*S-1-1-0"]).output();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
        }
    }
}

fn finish(output: std::process::Output) -> Run {
    Run {
        code: output.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// Asserts that standard output is exactly `expected` and the exit code is 0.
pub fn expect(run: &Run, expected: &str) {
    assert_eq!(run.out, expected, "stderr was: {}", run.err);
    assert_eq!(run.code, 0, "stderr was: {}", run.err);
}

//! The names a team shares: a bank, a task and a role. A bank name becomes a folder name under
//! the banks folder of the index home, so a name must never be able to leave it. Task and role
//! names end up inside tags, and follow the same rule so that one check covers all three. A bank
//! name must also be a folder name of its own on every system: it cannot end in a dot or be the
//! name of a Windows device.

use crate::fail::{Fail, Res};

/// The longest name, in characters.
const MAX_LEN: usize = 64;

/// The role of a call that names none.
pub const DEFAULT_ROLE: &str = "agent";

/// What a name is for. Only the words of the error message differ.
#[derive(Clone, Copy, Debug)]
pub enum Kind {
    Bank,
    Task,
    Role,
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Kind::Bank => "bank",
            Kind::Task => "task",
            Kind::Role => "role",
        }
    }
}

/// True for 1 to 64 characters out of `A-Z a-z 0-9 . _ -`, other than `.` and `..`.
pub fn is_valid(name: &str) -> bool {
    (1..=MAX_LEN).contains(&name.len())
        && name != "."
        && name != ".."
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// True for a name that Windows keeps for a device (`CON`, `NUL`, `COM1`, ...), with or without an
/// extension: the folder of such a bank cannot be made there.
fn is_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    match stem.as_bytes() {
        b"CON" | b"PRN" | b"AUX" | b"NUL" => true,
        [b'C', b'O', b'M', b'1'..=b'9'] | [b'L', b'P', b'T', b'1'..=b'9'] => true,
        _ => false,
    }
}

/// True for a name that is also the name of a folder of its own on every system. Windows drops the
/// dots at the end of a folder name, so `x.` would be the folder of `x` and `x..` and `...` would
/// have no folder, and it keeps the names of devices. The rule of the contract lets these pass.
fn is_folder_name(name: &str) -> bool {
    !name.ends_with('.') && !is_device_name(name)
}

/// The name itself, or a usage error that says which kind of name it was not. A bank name has to
/// be a folder name as well; a task and a role end up inside a tag and have the one rule.
pub fn check(kind: Kind, name: &str) -> Res<String> {
    let folder = !matches!(kind, Kind::Bank) || is_folder_name(name);
    if is_valid(name) && folder {
        Ok(name.to_string())
    } else {
        Err(Fail::usage(format!("{name} is not a valid {} name", kind.word())))
    }
}

/// The value of an environment variable, when it is set and not empty. An empty variable counts
/// as not set, as it always did for `PGR_BANK`.
pub fn from_env(variable: &str) -> Option<String> {
    std::env::var(variable).ok().filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_names_pass() {
        for name in ["team", "Team-1", "a.b_c", "x", &"n".repeat(64), "..x", "x..", "v1.2"] {
            assert!(is_valid(name), "{name}");
        }
    }

    #[test]
    fn names_that_could_leave_the_folder_or_are_empty_do_not() {
        for name in ["", ".", "..", "..\\x", "../x", "a\\b", "a/b", "c:", "a b", "a:b", "é", &"n".repeat(65), "a\0b"] {
            assert!(!is_valid(name), "{name:?}");
        }
    }

    #[test]
    fn a_bank_name_is_also_the_name_of_a_folder_of_its_own_on_every_system() {
        for name in ["x.", "x..", "...", "....", "NUL", "nul", "Con", "aux.txt", "COM1", "lpt9", "prn", "Com5.log"] {
            assert!(is_valid(name), "{name}: the contract's rule lets it pass");
            assert_eq!(check(Kind::Bank, name).unwrap_err().text(), format!("pgr: {name} is not a valid bank name"));
            // A task and a role end up inside a tag, not in a folder name.
            assert_eq!(check(Kind::Task, name).unwrap(), name);
            assert_eq!(check(Kind::Role, name).unwrap(), name);
        }
        for name in ["console", "com10", "nulls", "x.y", ".x", "lpt", "a-b_c", "..x", "v1.2"] {
            assert_eq!(check(Kind::Bank, name).unwrap(), name);
        }
    }

    #[test]
    fn the_error_names_the_kind_and_the_value() {
        assert_eq!(check(Kind::Bank, "a\\b").unwrap_err().text(), "pgr: a\\b is not a valid bank name");
        assert_eq!(check(Kind::Task, "").unwrap_err().code(), 2);
        assert_eq!(check(Kind::Role, "r..le").unwrap(), "r..le");
        assert!(check(Kind::Role, "x/y").unwrap_err().text().ends_with("is not a valid role name"));
    }
}

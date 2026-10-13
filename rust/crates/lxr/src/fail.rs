//! How a command stops before its answer is complete, and the exit code that goes with it.

/// A reason to stop. Every variant maps to one exit code of the command line contract.
#[derive(Debug)]
pub enum Fail {
    /// Not an error: pgr is switched off for this folder. The line goes to standard
    /// output and the exit code is 0.
    Off(String),
    /// Wrong arguments, or an engine that was turned off. Exit code 2.
    Usage(String),
    /// No command was given. The short help goes to standard error, exit code 2.
    NoCommand,
    /// The index cannot be read or built. Exit code 1.
    Index(String),
}

/// The result type of every step that can stop the command.
pub type Res<T> = Result<T, Fail>;

impl Fail {
    /// A usage error with the given message.
    pub fn usage(message: impl Into<String>) -> Fail {
        Fail::Usage(message.into())
    }

    /// An index error with the given message.
    pub fn index(message: impl Into<String>) -> Fail {
        Fail::Index(message.into())
    }

    /// The message as it is printed, with the `pgr: ` prefix.
    pub fn text(&self) -> String {
        match self {
            Fail::Off(message) | Fail::Usage(message) | Fail::Index(message) => format!("pgr: {message}"),
            Fail::NoCommand => "pgr: no command given (try: pgr help)".to_string(),
        }
    }

    /// The process exit code for this failure.
    pub fn code(&self) -> i32 {
        match self {
            Fail::Off(_) => 0,
            Fail::Usage(_) | Fail::NoCommand => 2,
            Fail::Index(_) => 1,
        }
    }
}

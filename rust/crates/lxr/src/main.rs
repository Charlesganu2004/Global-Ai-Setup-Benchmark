//! pgr: a code graph, hybrid code search and memory for the repository a coding agent
//! works in. One program, started once per question, so start-up cost comes first:
//! every part of the index is read only when a command needs it.

/// Appends one formatted line to a `String`. Writing to a string cannot fail.
macro_rules! ln {
    ($out:expr, $($arg:tt)*) => {{
        use std::fmt::Write as _;
        let _ = writeln!($out, $($arg)*);
    }};
}

mod app;
mod ask;
mod cli;
mod ctx;
mod dates;
mod fail;
mod findcmd;
mod graphcmd;
mod guard;
mod index;
mod mcp;
mod memcmd;
mod pack;
mod rows;

use cli::Command;
use ctx::Ctx;
use fail::{Fail, Res};
use std::io::Write;

/// Writes a diagnostic line to standard error. A closed standard error is ignored.
pub fn say(line: &str) {
    let _ = writeln!(std::io::stderr(), "{line}");
}

/// Writes text to standard output. A closed pipe is not an error worth a message.
fn print(text: &str) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(text.as_bytes()).and_then(|_| out.flush());
}

/// Runs the command line and gives the exit code.
fn run() -> Result<(), Fail> {
    let cli = cli::parse(std::env::args_os().skip(1))?;
    match &cli.command {
        Command::Help => {
            print(cli::HELP);
            return Ok(());
        }
        Command::Version => {
            print(&format!("pgr {}\n", env!("CARGO_PKG_VERSION")));
            return Ok(());
        }
        _ => {}
    }
    let engines = cli.global.engines()?;
    if let Some(engine) = cli.command.engine() {
        engines.require(engine)?;
    }
    let root = Ctx::resolve_root(cli.global.root.as_deref())?;
    let mut ctx = Ctx::new(root, engines)?;
    if matches!(cli.command, Command::Mcp) {
        mcp::serve(&mut ctx);
        return Ok(());
    }
    let text = app::dispatch(&mut ctx, &cli.command)?;
    print(&text);
    Ok(())
}

/// Reports a failure the way the contract says and gives the exit code.
fn report(fail: &Fail) -> i32 {
    match fail {
        Fail::Off(message) => print(&format!("pgr: {message}\n")),
        Fail::NoCommand => {
            say(cli::HELP.trim_end());
        }
        other => say(&other.text()),
    }
    fail.code()
}

fn main() {
    let result: Res<()> = run();
    let code = match result {
        Ok(()) => 0,
        Err(fail) => report(&fail),
    };
    std::process::exit(code);
}

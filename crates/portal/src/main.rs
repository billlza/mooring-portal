//! `mooring-portal`: the server and the operator's commands.

use std::process::ExitCode;

use portal_core::time::SystemClock;

fn main() -> ExitCode {
    // Unlocked handles: each write takes the lock briefly. Holding
    // `stderr().lock()` here for the whole run would deadlock every log
    // line written from a runtime worker thread.
    mooring_portal::run(
        std::env::args_os(),
        &SystemClock,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

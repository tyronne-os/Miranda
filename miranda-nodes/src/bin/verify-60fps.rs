//! WO-3 T9 verification CLI.
//!
//! Runs the 60 FPS face dispatcher against a real `/dev/shm` mapping,
//! consumes the frames from the other side, and prints the acceptance report.
//! Exits non-zero if any criterion fails, so it can gate a build.
//!
//! ```text
//! cargo run --release -p miranda-nodes --bin verify-60fps -- [seconds] [seed]
//! ```
//!
//! Build in release. A debug build measures an unoptimised frame cost, which
//! says nothing about the shipped pipeline and would report a misleading
//! percentage of the budget.

use std::process::ExitCode;
use std::time::Duration;

use miranda_nodes::verify::{self, VerificationConfig};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);

    let seconds: u64 = match args.next() {
        Some(a) => match a.parse() {
            Ok(v) if v > 0 => v,
            _ => {
                eprintln!("usage: verify-60fps [seconds] [seed]");
                return ExitCode::from(2);
            }
        },
        None => 30,
    };

    let config = VerificationConfig {
        duration: Duration::from_secs(seconds),
        seed: args
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(VerificationConfig::default().seed),
        ..Default::default()
    };

    if cfg!(debug_assertions) {
        eprintln!(
            "WARNING: debug build. Frame-cost figures below are not \
             representative. Re-run with --release.\n"
        );
    }

    println!(
        "running {seconds}s verification, seed {}...\n",
        config.seed
    );

    let report = match verify::run(&config) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("verification could not run: {e}");
            return ExitCode::FAILURE;
        }
    };

    print!("{}", report.render());

    if report.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

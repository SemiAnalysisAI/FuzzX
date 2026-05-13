//! Turn an AFL++ corpus/crash file (raw bytes) back into the PTX text
//! that ptxas actually sees, and optionally run ptxas on it.
//!
//! AFL only persists the *raw* pre-mutator bytes; the byte→PTX
//! transform happens inside our mutator at fuzz time. So to reproduce
//! a saved crash you have to re-apply that transform here.
//!
//! Usage:
//!   ptx-fuzz-repro <path>          # print PTX to stdout
//!   ptx-fuzz-repro <path> --run    # also exec ptxas on it
//!
//! Honors $PTXAS for the ptxas path (default: `ptxas` in $PATH).

use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let path = match args.next() {
        Some(p) => p,
        None => {
            eprintln!("usage: ptx-fuzz-repro <path> [--run]");
            return ExitCode::from(2);
        }
    };
    let run = args.any(|a| a == "--run");

    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("ptx-fuzz-repro: cannot read {path}: {e}");
            return ExitCode::from(2);
        }
    };

    let ptx = ptx_fuzz_gen::generate_ptx(&bytes);

    if !run {
        print!("{ptx}");
        return ExitCode::from(0);
    }

    let tmp = match tempfile::NamedTempFile::with_suffix(".ptx") {
        Ok(f) => f,
        Err(e) => {
            eprintln!("ptx-fuzz-repro: tempfile: {e}");
            return ExitCode::from(2);
        }
    };
    if let Err(e) = std::fs::write(tmp.path(), &ptx) {
        eprintln!("ptx-fuzz-repro: write tempfile: {e}");
        return ExitCode::from(2);
    }

    let ptxas = std::env::var("PTXAS").unwrap_or_else(|_| "ptxas".to_string());
    let status = match Command::new(&ptxas).arg(tmp.path()).status() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ptx-fuzz-repro: spawning {ptxas}: {e}");
            return ExitCode::from(2);
        }
    };

    if let Some(code) = status.code() {
        eprintln!("ptx-fuzz-repro: ptxas exited with {code}");
        ExitCode::from(code.clamp(0, 255) as u8)
    } else {
        // Killed by signal — the interesting case.
        eprintln!("ptx-fuzz-repro: ptxas killed by signal");
        ExitCode::from(128)
    }
}

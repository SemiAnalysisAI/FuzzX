//! Stand-in for the real `ptxas` so the pipeline can be exercised on
//! systems without the CUDA toolkit installed.
//!
//! Behavior:
//!   * Reads a single positional path argument (matching real ptxas's
//!     command line) — or stdin if no path is given.
//!   * Walks the input, doing some token-level "parsing" so there's
//!     interesting branch structure for Valgrind to observe.
//!   * Crashes deliberately on a small set of input patterns, so the
//!     fuzzer has something to find. The patterns are documented below.
//!
//! Seeded crashes (a coverage-guided fuzzer should be able to find at
//! least the first):
//!   1. The byte sequence `@!` triggers a `null-pointer deref`
//!      simulation via `std::ptr::null_mut::<u8>().write(0)`.
//!   2. A string with more than 100 nested `{`s aborts (stack-overflow
//!      analogue).
//!
//! Real ptxas is, of course, far more complex; the goal here is just
//! to verify that the fuzzer plumbing finds *something*.

use std::env;
use std::fs;
use std::io::{self, Read};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let input_path = args.iter().skip(1).find(|a| !a.starts_with('-'));

    let input = match input_path {
        Some(p) => match fs::read_to_string(p) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("fake-ptxas: cannot read {p}: {e}");
                return ExitCode::from(1);
            }
        },
        None => {
            let mut buf = String::new();
            if io::stdin().read_to_string(&mut buf).is_err() {
                eprintln!("fake-ptxas: cannot read stdin");
                return ExitCode::from(1);
            }
            buf
        }
    };

    match parse(&input) {
        Ok(()) => ExitCode::from(0),
        Err(msg) => {
            eprintln!("fake-ptxas: {msg}");
            ExitCode::from(1)
        }
    }
}

fn parse(s: &str) -> Result<(), String> {
    let mut depth = 0u32;
    let mut prev = '\0';
    for c in s.chars() {
        match c {
            '{' => {
                depth += 1;
                if depth > 100 {
                    // Seeded "crash" #2: simulate a stack-overflow with
                    // an abort (signal 6).
                    std::process::abort();
                }
            }
            '}' => {
                if depth == 0 {
                    return Err("unexpected '}'".to_string());
                }
                depth -= 1;
            }
            '!' if prev == '@' => {
                // Seeded crash #1: write through a pointer LLVM can't
                // prove is null at compile time. (Writing through a
                // *literal* null is UB the optimizer happily deletes —
                // even in release builds — so we have to obscure the
                // origin of the pointer.)
                let bad: usize = std::hint::black_box(0);
                unsafe { (bad as *mut u8).write(0) };
            }
            _ => {}
        }
        prev = c;
    }
    if depth != 0 {
        return Err(format!("unbalanced: {depth} unclosed '{{'"));
    }
    Ok(())
}

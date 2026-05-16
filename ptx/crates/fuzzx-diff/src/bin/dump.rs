//! Tiny helper: print the PTX kernel generated for a given seed. Useful when
//! eyeballing changes to the generator.
//!
//! Usage: `cargo run --release --bin fuzzx-diff-dump-gen -- <seed-decimal>`

use fuzzx_execgen::{bytes_from_seed, generate_from_bytes};

fn main() {
    let seed: u64 = std::env::args()
        .nth(1)
        .map(|s| s.parse().expect("seed must be a u64"))
        .unwrap_or(7);
    let program_bytes = std::env::var("DIV_PROGRAM_BYTES")
        .ok()
        .map(|s| s.parse().expect("DIV_PROGRAM_BYTES must be a usize"))
        .unwrap_or(4096);
    let bytes = bytes_from_seed(seed, program_bytes);
    let ptx = generate_from_bytes(&bytes).expect("generate");
    print!("{ptx}");
}

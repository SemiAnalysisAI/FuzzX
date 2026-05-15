//! Tiny helper: print the PTX kernel generated for a given seed. Useful when
//! eyeballing changes to the generator.
//!
//! Usage: `cargo run --release --bin dump_gen -- <seed-decimal>`

use ptx_fuzz_execgen::{bytes_from_seed, generate_from_bytes};

fn main() {
    let seed: u64 = std::env::args()
        .nth(1)
        .map(|s| s.parse().expect("seed must be a u64"))
        .unwrap_or(7);
    let bytes = bytes_from_seed(seed, 4096);
    let ptx = generate_from_bytes(&bytes).expect("generate");
    print!("{ptx}");
}

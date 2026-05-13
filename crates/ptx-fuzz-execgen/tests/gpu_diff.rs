//! Self-test: every generated program should `matches()` (both opt levels
//! compile, both launch, and the outputs are bit-identical). Any failure here
//! — diverged OR both-failed — means the generator violated one of its own
//! invariants (race, UB, OOB memory, non-termination, asymmetric ptxas
//! accept) and we should fix the generator before trusting any real divergence.

use ptx_fuzz_exec::{differential, Cuda};
use ptx_fuzz_execgen::{
    bytes_from_seed, generate_from_bytes, input_for_seed, output_len, KERNEL_NAME, N_THREADS,
    TARGET_ARCH,
};

const SEEDS: u64 = 200;
const PROGRAM_BYTES: usize = 4096;

#[test]
fn every_generated_kernel_matches_at_o0_and_o3() {
    let cuda = Cuda::init(0).expect("Cuda::init");
    let arch = format!("-arch={TARGET_ARCH}");

    let mut bad: Vec<(u64, &'static str, String)> = Vec::new();

    for seed in 0..SEEDS {
        let bytes = bytes_from_seed(seed, PROGRAM_BYTES);
        let ptx = match generate_from_bytes(&bytes) {
            Ok(p) => p,
            Err(_) => continue, // out of entropy, skip
        };
        let input = input_for_seed(seed);
        let out = differential(
            &cuda,
            &ptx,
            &arch,
            KERNEL_NAME,
            (1, 1, 1),
            (N_THREADS, 1, 1),
            &input,
            output_len(),
            N_THREADS,
        );
        if !out.matches() {
            let kind = if out.diverged() { "diverged" } else { "both_failed" };
            let detail = format!(
                "o0={}\no3={}",
                match &out.o0 { Ok(b) => format!("ok ({} bytes)", b.len()), Err(e) => format!("err: {e}") },
                match &out.o3 { Ok(b) => format!("ok ({} bytes)", b.len()), Err(e) => format!("err: {e}") },
            );
            bad.push((seed, kind, detail));
        }
    }

    assert!(
        bad.is_empty(),
        "{} / {SEEDS} generated kernels failed (first 3 shown):\n{}",
        bad.len(),
        bad.iter()
            .take(3)
            .map(|(seed, kind, detail)| format!("seed {seed} ({kind}):\n{detail}"))
            .collect::<Vec<_>>()
            .join("\n---\n"),
    );
}

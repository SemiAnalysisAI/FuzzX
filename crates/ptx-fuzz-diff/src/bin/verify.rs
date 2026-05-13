//! Re-run a saved divergence to confirm it's deterministic and reproducible.
//!
//! Compiles the saved `program.ptx` at -O0 and -O3 multiple times, runs each
//! a few times with the saved input, and reports:
//!   * Whether -O0 vs -O3 disagree (the divergence)
//!   * Whether each opt level is deterministic across repeated runs
//!     (rules out flakiness from race conditions or our own side)
//!
//! Usage: `ptx-fuzz-diff-verify <div-dir>`

use std::env;
use std::path::PathBuf;

use anyhow::{anyhow, Context as _, Result};
use ptx_fuzz_exec::{compile, Cuda};
use ptx_fuzz_execgen::{output_len, KERNEL_NAME, N_THREADS, TARGET_ARCH};

const REPEATS: usize = 5;

fn run(cuda: &Cuda, cubin: &[u8], input: &[u8]) -> Result<Vec<u8>> {
    cuda.launch(
        cubin,
        KERNEL_NAME,
        (1, 1, 1),
        (N_THREADS, 1, 1),
        input,
        output_len(),
        N_THREADS,
    )
}

fn main() -> Result<()> {
    let dir: PathBuf = env::args()
        .nth(1)
        .ok_or_else(|| anyhow!("usage: ptx-fuzz-diff-verify <div-dir>"))?
        .into();
    let ptx = std::fs::read_to_string(dir.join("program.ptx"))
        .with_context(|| format!("reading program.ptx in {}", dir.display()))?;
    let input = std::fs::read(dir.join("input.bin")).context("reading input.bin")?;

    let cuda = Cuda::init(0).context("Cuda::init")?;
    let arch = format!("-arch={TARGET_ARCH}");

    let cubin_o0 = compile(&ptx, &[arch.as_str(), "-O0"]).context("compile -O0")?;
    let cubin_o3 = compile(&ptx, &[arch.as_str(), "-O3"]).context("compile -O3")?;

    println!("-- self-consistency (same opt level, repeated runs) --");
    for (label, cubin) in [("-O0", &cubin_o0), ("-O3", &cubin_o3)] {
        let first = run(&cuda, cubin, &input)?;
        let mut all_same = true;
        for _ in 1..REPEATS {
            let next = run(&cuda, cubin, &input)?;
            if next != first {
                all_same = false;
            }
        }
        println!(
            "  {label}: {} runs, deterministic={}",
            REPEATS,
            if all_same { "yes" } else { "NO (this is a real problem)" },
        );
    }

    println!();
    println!("-- recompile-stability (recompile each iter, same opt level) --");
    for (label, opt) in [("-O0", "-O0"), ("-O3", "-O3")] {
        let mut first: Option<Vec<u8>> = None;
        let mut all_same = true;
        for _ in 0..REPEATS {
            let c = compile(&ptx, &[arch.as_str(), opt])?;
            let r = run(&cuda, &c, &input)?;
            match &first {
                None => first = Some(r),
                Some(f) if f == &r => {}
                Some(_) => all_same = false,
            }
        }
        println!(
            "  {label}: {} compile+run cycles, stable={}",
            REPEATS,
            if all_same { "yes" } else { "NO (ptxas itself is nondeterministic)" },
        );
    }

    println!();
    println!("-- -O0 vs -O3 (the divergence) --");
    let r0 = run(&cuda, &cubin_o0, &input)?;
    let r3 = run(&cuda, &cubin_o3, &input)?;
    if r0 == r3 {
        println!("  outputs MATCH — bug not reproduced");
    } else {
        let n_threads = N_THREADS as usize;
        let k = output_len() / n_threads / 4;
        let to_u32s = |b: &[u8]| -> Vec<u32> {
            b.chunks_exact(4).map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]])).collect()
        };
        let u0 = to_u32s(&r0);
        let u3 = to_u32s(&r3);
        let differing: Vec<usize> = (0..n_threads)
            .filter(|&t| u0[t * k..t * k + k] != u3[t * k..t * k + k])
            .collect();
        println!(
            "  outputs DIFFER on {} of {n_threads} threads: tids {:?}",
            differing.len(),
            differing,
        );
    }

    Ok(())
}

//! Quick "does this PTX still diverge?" test for interactive manual reduction.
//! By default, two runs at each opt level must be bit-identical within each
//! opt level, and outputs must differ between opt levels.
//!
//! Usage: `fuzzx-diff-test [--quick] <ptx-file> <input.bin>`
//!   exit 0 — diverges (`--quick`) or deterministically diverges (default)
//!   exit 1 — does NOT (matches, both-fail, non-determ, or compile/launch fail)
//! Prints a one-line verdict.
//!
//! `--quick` runs each opt level once. Use it for fast candidate screening, then
//! re-run without `--quick` before keeping or reporting a reduction.

use std::path::PathBuf;

use anyhow::{anyhow, Context as _, Result};
use fuzzx_exec::{compile, Cuda, CudaBuffers};
use fuzzx_execgen::{output_len, KERNEL_NAME, N_THREADS, TARGET_ARCH};

fn compile_at(ptx: &str, opt: &str) -> Result<Vec<u8>> {
    let arch = format!("-arch={TARGET_ARCH}");
    compile(ptx, &[arch.as_str(), opt])
}

fn launch_cubin(cuda: &Cuda, bufs: &CudaBuffers, cubin: &[u8], input: &[u8]) -> Result<Vec<u8>> {
    cuda.launch_with(
        bufs,
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
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let quick = args.first().is_some_and(|arg| arg == "--quick");
    if quick {
        args.remove(0);
    }
    let ptx_path = PathBuf::from(
        args.first()
            .ok_or_else(|| anyhow!("usage: fuzzx-diff-test [--quick] <ptx> <input.bin>"))?,
    );
    let in_path = PathBuf::from(
        args.get(1)
            .ok_or_else(|| anyhow!("usage: fuzzx-diff-test [--quick] <ptx> <input.bin>"))?,
    );
    let ptx = std::fs::read_to_string(&ptx_path).with_context(|| ptx_path.display().to_string())?;
    let input = std::fs::read(&in_path).with_context(|| in_path.display().to_string())?;

    let cuda = Cuda::init(0).context("Cuda::init")?;
    let bufs = cuda.alloc_buffers(input.len(), output_len())?;

    let o0_cubin = match compile_at(&ptx, "-O0") {
        Ok(c) => c,
        Err(e) => {
            println!("FAIL -O0 compile: {e:#}");
            std::process::exit(1);
        }
    };
    let o0a = match launch_cubin(&cuda, &bufs, &o0_cubin, &input) {
        Ok(b) => b,
        Err(e) => {
            println!("FAIL -O0 launch: {e:#}");
            std::process::exit(1);
        }
    };
    if !quick {
        let o0b = launch_cubin(&cuda, &bufs, &o0_cubin, &input).context("re-run -O0")?;
        if o0a != o0b {
            println!("FAIL -O0 non-deterministic");
            std::process::exit(1);
        }
    }
    let o3_cubin = match compile_at(&ptx, "-O3") {
        Ok(c) => c,
        Err(e) => {
            println!("FAIL -O3 compile: {e:#}");
            std::process::exit(1);
        }
    };
    let o3a = match launch_cubin(&cuda, &bufs, &o3_cubin, &input) {
        Ok(b) => b,
        Err(e) => {
            println!("FAIL -O3 launch: {e:#}");
            std::process::exit(1);
        }
    };
    if !quick {
        let o3b = launch_cubin(&cuda, &bufs, &o3_cubin, &input).context("re-run -O3")?;
        if o3a != o3b {
            println!("FAIL -O3 non-deterministic");
            std::process::exit(1);
        }
    }
    if o0a == o3a {
        println!("FAIL outputs match (no divergence)");
        std::process::exit(1);
    }

    let n_diff = o0a
        .chunks_exact(4)
        .zip(o3a.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    let n_tids = o0a
        .chunks(16)
        .zip(o3a.chunks(16))
        .filter(|(a, b)| a != b)
        .count();
    println!(
        "DIVERGES{} — {n_tids}/{} tids differ, {n_diff}/128 u32 slots differ",
        if quick {
            " (quick)"
        } else {
            " (deterministic)"
        },
        N_THREADS,
    );
    Ok(())
}

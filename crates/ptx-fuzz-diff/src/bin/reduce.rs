//! Delta-debug a saved divergence: shrink the PTX while preserving the
//! invariants that make any divergence a real ptxas miscompile rather than UB.
//!
//! Strategy: only remove "body" instructions (between `bra block_0;` and
//! `exit:`) plus epilogue store lines. The prologue and the address
//! arithmetic in the epilogue (cvta / mul.wide / add.s64) are sacrosanct —
//! losing them would make the kernel race on a shared output address or read
//! OOB.
//!
//! Each candidate removal must:
//!   * ptxas-accept at both `-O0` and `-O3`;
//!   * launch successfully at both opt levels;
//!   * still produce divergent outputs;
//!   * be DETERMINISTIC at each opt level (two runs yield identical bits) —
//!     this rejects removals that introduce a race we hadn't noticed.
//!
//! Usage: `ptx-fuzz-diff-reduce <div-dir>`
//! Output: `reduced.ptx` (+ outputs) in the dir.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{anyhow, bail, Context as _, Result};
use ptx_fuzz_exec::{compile, Cuda};
use ptx_fuzz_execgen::{output_len, KERNEL_NAME, N_THREADS, TARGET_ARCH};

fn run_at(cuda: &Cuda, ptx: &str, input: &[u8], opt: &str) -> Result<Vec<u8>> {
    let arch = format!("-arch={TARGET_ARCH}");
    let cubin = compile(ptx, &[arch.as_str(), opt])?;
    cuda.launch(
        &cubin,
        KERNEL_NAME,
        (1, 1, 1),
        (N_THREADS, 1, 1),
        input,
        output_len(),
        N_THREADS,
    )
}

/// Both opt levels compile + launch + are deterministic across two runs, and
/// their outputs differ. Returns the divergent (o0, o3) on success.
fn diverges_deterministically(cuda: &Cuda, ptx: &str, input: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let o0a = run_at(cuda, ptx, input, "-O0").ok()?;
    let o0b = run_at(cuda, ptx, input, "-O0").ok()?;
    if o0a != o0b { return None; }
    let o3a = run_at(cuda, ptx, input, "-O3").ok()?;
    let o3b = run_at(cuda, ptx, input, "-O3").ok()?;
    if o3a != o3b { return None; }
    if o0a == o3a { return None; }
    Some((o0a, o3a))
}

/// Indices into `ptx.lines()` that the reducer is allowed to try removing.
/// Sacrosanct: prologue (everything up to and including `bra block_0;`),
/// epilogue address arithmetic, labels, structural lines, `ret;`, braces.
fn removable_indices(ptx: &str) -> Result<Vec<usize>> {
    let lines: Vec<&str> = ptx.lines().collect();
    let prologue_end = lines
        .iter()
        .position(|l| {
            let t = l.trim();
            t.starts_with("bra") && t.contains("block_0;")
        })
        .ok_or_else(|| anyhow!("could not locate `bra block_0;` to mark end of prologue"))?;
    let exit_start = lines
        .iter()
        .position(|l| l.trim() == "exit:")
        .ok_or_else(|| anyhow!("could not locate `exit:` label"))?;

    let mut out = Vec::new();
    // Body: between prologue_end (exclusive) and exit_start (exclusive).
    for i in (prologue_end + 1)..exit_start {
        let t = lines[i].trim();
        if t.is_empty() || t.ends_with(':') {
            continue;
        }
        out.push(i);
    }
    // Epilogue: store lines only; address arithmetic and `ret;` stay put.
    for i in exit_start..lines.len() {
        if lines[i].trim().starts_with("st.global.u32") {
            out.push(i);
        }
    }
    Ok(out)
}

fn main() -> Result<()> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or_else(|| anyhow!("usage: ptx-fuzz-diff-reduce <div-dir>"))?,
    );
    let mut ptx = std::fs::read_to_string(dir.join("program.ptx"))
        .with_context(|| format!("reading program.ptx in {}", dir.display()))?;
    let input = std::fs::read(dir.join("input.bin")).context("input.bin")?;

    let cuda = Cuda::init(0).context("Cuda::init")?;
    let start_lines = ptx.lines().count();
    let start_body = removable_indices(&ptx)?.len();

    if diverges_deterministically(&cuda, &ptx, &input).is_none() {
        bail!("starting PTX does not deterministically diverge — nothing to reduce");
    }
    eprintln!("starting at {start_lines} total lines ({start_body} removable candidates)");

    let t0 = Instant::now();
    let mut total_removed = 0usize;
    loop {
        let candidates = removable_indices(&ptx)?;
        let lines: Vec<String> = ptx.lines().map(str::to_string).collect();
        let mut progress = false;
        // Bottom-up: removing later lines is less likely to cascade into
        // use-before-def in code that hasn't run yet.
        for &i in candidates.iter().rev() {
            let mut candidate_lines = lines.clone();
            candidate_lines.remove(i);
            let candidate = candidate_lines.join("\n");
            if diverges_deterministically(&cuda, &candidate, &input).is_some() {
                eprintln!(
                    "  removed line {i:3} ({} → {} lines): {}",
                    lines.len(),
                    lines.len() - 1,
                    lines[i].trim(),
                );
                ptx = candidate;
                progress = true;
                total_removed += 1;
                break;
            }
        }
        if !progress {
            break;
        }
    }

    let end_lines = ptx.lines().count();
    let elapsed = t0.elapsed().as_secs_f64();
    eprintln!(
        "reduced {start_lines} → {end_lines} lines ({} removed) in {:.1}s",
        total_removed, elapsed,
    );

    std::fs::write(dir.join("reduced.ptx"), &ptx)?;
    if let Some((o0, o3)) = diverges_deterministically(&cuda, &ptx, &input) {
        std::fs::write(dir.join("reduced_o0.bin"), &o0)?;
        std::fs::write(dir.join("reduced_o3.bin"), &o3)?;
        eprintln!("saved reduced.ptx, reduced_o0.bin, reduced_o3.bin in {}", dir.display());
    } else {
        bail!("reduced PTX no longer diverges (bug in reducer?)");
    }
    Ok(())
}

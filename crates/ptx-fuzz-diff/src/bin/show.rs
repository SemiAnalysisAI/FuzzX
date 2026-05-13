//! Render a saved divergence directory into a human-readable report.
//!
//! Usage: `ptx-fuzz-diff-show <divergences-dir>/div-...-...`
//! Prints a per-thread diff of the two u32 output buffers and dumps the PTX.

use std::env;
use std::path::Path;

use anyhow::{bail, Context as _, Result};
use ptx_fuzz_execgen::{N_OUTPUTS, N_THREADS};

fn read_u32s(path: &Path) -> Result<Vec<u32>> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if bytes.len() % 4 != 0 {
        bail!("{} length {} is not a multiple of 4", path.display(), bytes.len());
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

fn read_str(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn main() -> Result<()> {
    let dir = env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("usage: ptx-fuzz-diff-show <div-dir>"))?;

    let summary = read_str(&dir.join("summary.txt")).unwrap_or_default();
    println!("=== summary ===");
    println!("{}", summary.trim());
    println!();

    let input = read_u32s(&dir.join("input.bin")).ok();
    let o0 = read_u32s(&dir.join("output_o0.bin")).ok();
    let o3 = read_u32s(&dir.join("output_o3.bin")).ok();
    if let Some(e) = read_str(&dir.join("output_o0.err")) {
        println!("=== output_o0.err ===\n{}\n", e.trim());
    }
    if let Some(e) = read_str(&dir.join("output_o3.err")) {
        println!("=== output_o3.err ===\n{}\n", e.trim());
    }

    if let (Some(o0), Some(o3)) = (o0.as_ref(), o3.as_ref()) {
        println!("=== per-thread diff (input, o0[0..4], o3[0..4]; '!' = differ) ===");
        let k = N_OUTPUTS as usize;
        for tid in 0..N_THREADS as usize {
            let i = input.as_ref().map(|v| v[tid]).unwrap_or(0);
            let a = &o0[tid * k..tid * k + k];
            let b = &o3[tid * k..tid * k + k];
            let mark = if a == b { " " } else { "!" };
            print!("{mark}t{tid:02} in={i:#010x}  o0=");
            for v in a { print!("{v:#010x} "); }
            print!(" o3=");
            for v in b { print!("{v:#010x} "); }
            println!();
        }
        let differing_threads = (0..N_THREADS as usize)
            .filter(|&tid| o0[tid * k..tid * k + k] != o3[tid * k..tid * k + k])
            .count();
        println!("\n{differing_threads} / {N_THREADS} threads diverged");
    }

    if let Some(ptx) = read_str(&dir.join("program.ptx")) {
        println!("\n=== program.ptx ===\n{ptx}");
    }
    Ok(())
}

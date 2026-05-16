//! Independent oracle for the seed-0x50f divergence.
//!
//! This binary contains a hand-translated Rust implementation of the PTX
//! kernel in `divergences-r1/div-1778654857-000000000000050f/program.ptx`.
//! It executes the same control flow and arithmetic per thread, with no
//! optimization (Rust at -O0 is irrelevant — we're not relying on its
//! compiler to do anything clever, just to faithfully execute the
//! instructions we wrote). The result is then compared against the saved
//! -O0 and -O3 GPU outputs.
//!
//! What this proves:
//!
//!   * If the simulation matches -O0 but not -O3, then -O3 is wrong:
//!     -O0's bits agree with the literal PTX semantics, -O3's don't.
//!
//!   * The simulator only touches integer ops with well-defined PTX
//!     semantics (no shift-by-≥32, no div, no FP, no memory), so it can't
//!     itself be exhibiting UB.
//!
//! The kernel only ever reaches blocks 0, 2, 3, 4, 5, 6 (block_1 is
//! statically unreachable in the CFG).

use std::path::PathBuf;

use anyhow::{anyhow, Context as _, Result};
use fuzzx_execgen::{output_len, N_OUTPUTS, N_THREADS};

/// Mirrors `program.ptx` exactly. See `program.ptx` next to this file for
/// the source; line-by-line the bodies of these match-arms correspond to
/// `block_0:`, `block_2:`, etc. Some assignments are dead (overwritten
/// before being read) — these mirror dead stores in the PTX itself, which
/// is what we're asserting -O3 must be equivalent to.
#[allow(unused_assignments)]
fn simulate(tid: u32, n: u32, in_val: u32) -> [u32; N_OUTPUTS as usize] {
    // Prologue initializers.
    let (mut r0, mut r1, mut r2, mut r3, mut r4, mut r5, mut r6, mut r7) =
        (n, tid, in_val, n, 4u32, tid, in_val, n);
    let (mut r9, mut r10) = (0u32, 2u32);

    let mut next: i32 = 0; // entry: bra block_0
    loop {
        match next {
            0 => {
                // block_0
                r2 = r0 ^ r4;
                r0 = !r6;
                r0 = r4.wrapping_shl(22);
                r0 = r2.wrapping_sub(17);
                let p0 = r5 <= r0;
                next = if p0 { 5 } else { 3 };
            }
            2 => {
                let p2 = r3 != r7;
                r2 = if p2 { r0 } else { r2 };
                r3 = r5.wrapping_add(r4);
                r7 = 7u32.wrapping_add(r4);
                r6 = r3.wrapping_shr(9);
                let p3 = r2 > r1;
                r0 = if p3 { 27 } else { r0 };
                r2 = r4.wrapping_add(r0);
                next = 4;
            }
            3 => {
                r4 = r4.wrapping_shr(19);
                r2 = 3 | r2;
                r4 = r7 ^ r2;
                next = 4;
            }
            4 => {
                r1 = 26u32.min(r3);
                r7 = r7.wrapping_shr(22);
                let p4 = r5 != r1;
                r5 = if p4 { r1 } else { 19 };
                r3 = r1.wrapping_add(r2); // dead — overwritten on next line
                r3 = r2 ^ r6;
                r0 = !6u32;
                let p5 = r9 == 0;
                if p5 {
                    next = 5; // loop_done_9
                } else {
                    r9 = r9.wrapping_sub(1);
                    next = 3;
                }
            }
            5 => {
                r4 = 22 | r5;
                let p6 = r4 >= r7;
                r4 = if p6 { r7 } else { r2 };
                r7 = r6.max(r3);
                r1 = r4.min(r5);
                r0 = (((24u64) * r1 as u64) >> 32) as u32; // mul.hi.u32
                r6 = !r4;
                let p7 = r10 == 0;
                if p7 {
                    next = 6; // loop_done_10
                } else {
                    r10 = r10.wrapping_sub(1);
                    next = 2;
                }
            }
            6 => {
                r5 = 21u32.wrapping_mul(r6);
                r1 = r5.max(r6); // dead — overwritten on next line
                r1 = ((r3 as u64 * 0u64) >> 32) as u32;
                let p8 = r3 >= r2;
                r3 = if p8 { r0 } else { r3 };
                let p9 = r3 <= r0;
                r2 = if p9 { r5 } else { r6 };
                let _r4 = r3.wrapping_sub(r1); // dead; not in outputs
                return [r0, r1, r2, r3];
            }
            _ => unreachable!(),
        }
    }
}

fn to_u32s(b: &[u8]) -> Vec<u32> {
    b.chunks_exact(4)
        .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn main() -> Result<()> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "divergences-r1/div-1778654857-000000000000050f".into()),
    );
    let input = std::fs::read(dir.join("input.bin")).context("input.bin")?;
    let o0 = std::fs::read(dir.join("output_o0.bin")).context("output_o0.bin")?;
    let o3 = std::fs::read(dir.join("output_o3.bin")).context("output_o3.bin")?;
    if input.len() != (N_THREADS as usize) * 4 {
        return Err(anyhow!(
            "input.bin length {} != expected {}",
            input.len(),
            N_THREADS * 4
        ));
    }
    if o0.len() != output_len() || o3.len() != output_len() {
        return Err(anyhow!("output length mismatch"));
    }

    let input_u = to_u32s(&input);
    let o0_u = to_u32s(&o0);
    let o3_u = to_u32s(&o3);
    let k = N_OUTPUTS as usize;

    let mut sim_matches_o0 = true;
    let mut sim_matches_o3 = true;
    let mut o0_disagrees: Vec<usize> = Vec::new();
    let mut o3_disagrees: Vec<usize> = Vec::new();

    println!("tid  input       sim                                     o0 match  o3 match");
    println!("---  ----------  --------------------------------------  --------  --------");
    for tid in 0..N_THREADS as usize {
        let sim = simulate(tid as u32, N_THREADS, input_u[tid]);
        let oa = &o0_u[tid * k..tid * k + k];
        let ob = &o3_u[tid * k..tid * k + k];
        let match_o0 = sim == oa;
        let match_o3 = sim == ob;
        if !match_o0 {
            sim_matches_o0 = false;
            o0_disagrees.push(tid);
        }
        if !match_o3 {
            sim_matches_o3 = false;
            o3_disagrees.push(tid);
        }
        println!(
            "t{tid:02}  {:#010x}  [{:#010x} {:#010x} {:#010x} {:#010x}]   {}        {}",
            input_u[tid],
            sim[0],
            sim[1],
            sim[2],
            sim[3],
            if match_o0 { "y" } else { "N" },
            if match_o3 { "y" } else { "N" },
        );
    }

    println!();
    println!(
        "simulation matches -O0 on all {} threads: {sim_matches_o0}",
        N_THREADS
    );
    println!(
        "simulation matches -O3 on all {} threads: {sim_matches_o3}",
        N_THREADS
    );
    if !sim_matches_o0 {
        println!("  -O0 disagrees on tids: {:?}", o0_disagrees);
    }
    if !sim_matches_o3 {
        println!("  -O3 disagrees on tids: {:?}", o3_disagrees);
    }

    if sim_matches_o0 && !sim_matches_o3 {
        println!();
        println!("VERDICT: -O3 is wrong. -O0 matches the literal PTX semantics; -O3 does not.");
    } else if !sim_matches_o0 && sim_matches_o3 {
        println!("\nVERDICT: -O0 is wrong (or the hand-translation has a bug).");
    } else if sim_matches_o0 && sim_matches_o3 {
        println!("\nNo divergence — both opt levels agree with simulation.");
    } else {
        println!("\nNeither matches simulation — likely a bug in the hand-translation.");
    }

    Ok(())
}

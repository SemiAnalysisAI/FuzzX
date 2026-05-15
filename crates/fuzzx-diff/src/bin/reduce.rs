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
//! Usage: `fuzzx-diff-reduce <div-dir>`
//! Output: `reduced.ptx` (+ outputs) in the dir.
//!
//! Parallelism:
//!   REDUCE_GPUS             default: DIV_GPUS, then all visible devices
//!   REDUCE_WORKERS_PER_GPU  default: DIV_WORKERS_PER_GPU, then roughly
//!                           one ptxas worker per host core across all GPUs
//!   REDUCE_NO_PROGRESS_SECS default: 120; fail fast if no candidate finishes

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context as _, Result};
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

fn parse_gpu_list(key: &str) -> Result<Option<Vec<i32>>> {
    match std::env::var(key) {
        Ok(s) => s
            .split(',')
            .map(|t| {
                t.trim()
                    .parse::<i32>()
                    .map_err(|e| anyhow!("{key} entry {t:?}: {e}"))
            })
            .collect::<Result<Vec<_>>>()
            .map(Some),
        Err(_) => Ok(None),
    }
}

fn parse_gpus() -> Result<Vec<i32>> {
    if let Some(gpus) = parse_gpu_list("REDUCE_GPUS")? {
        if gpus.is_empty() {
            bail!("REDUCE_GPUS must not be empty");
        }
        return Ok(gpus);
    }
    if let Some(gpus) = parse_gpu_list("DIV_GPUS")? {
        if gpus.is_empty() {
            bail!("DIV_GPUS must not be empty");
        }
        return Ok(gpus);
    }
    let n = Cuda::device_count().context("Cuda::device_count")?;
    if n <= 0 {
        bail!("no CUDA devices visible");
    }
    Ok((0..n).collect())
}

fn env_usize(key: &str) -> Result<Option<usize>> {
    match std::env::var(key) {
        Ok(s) => s
            .parse::<usize>()
            .map(Some)
            .map_err(|e| anyhow!("env {key}={s:?} parse error: {e}")),
        Err(_) => Ok(None),
    }
}

fn default_workers_per_gpu(n_gpus: usize) -> usize {
    let cores = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    ((cores + n_gpus - 1) / n_gpus).clamp(1, 16)
}

fn workers_per_gpu(n_gpus: usize) -> Result<usize> {
    let n = env_usize("REDUCE_WORKERS_PER_GPU")?
        .or(env_usize("DIV_WORKERS_PER_GPU")?)
        .unwrap_or_else(|| default_workers_per_gpu(n_gpus));
    if n == 0 {
        bail!("workers per GPU must be nonzero");
    }
    Ok(n)
}

fn no_progress_timeout() -> Result<Duration> {
    Ok(Duration::from_secs(
        env_usize("REDUCE_NO_PROGRESS_SECS")?.unwrap_or(120) as u64,
    ))
}

fn candidate_without_line(lines: &[String], remove_idx: usize) -> String {
    let mut out = String::with_capacity(lines.iter().map(|l| l.len() + 1).sum());
    for (i, line) in lines.iter().enumerate() {
        if i != remove_idx {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Both opt levels compile + launch + are deterministic across two runs, and
/// their outputs differ. Returns the divergent (o0, o3) on success.
fn diverges_deterministically(
    cuda: &Cuda,
    bufs: &CudaBuffers,
    ptx: &str,
    input: &[u8],
) -> Option<(Vec<u8>, Vec<u8>)> {
    let o0_cubin = compile_at(ptx, "-O0").ok()?;
    let o0a = launch_cubin(cuda, bufs, &o0_cubin, input).ok()?;
    let o0b = launch_cubin(cuda, bufs, &o0_cubin, input).ok()?;
    if o0a != o0b {
        return None;
    }
    let o3_cubin = compile_at(ptx, "-O3").ok()?;
    let o3a = launch_cubin(cuda, bufs, &o3_cubin, input).ok()?;
    let o3b = launch_cubin(cuda, bufs, &o3_cubin, input).ok()?;
    if o3a != o3b {
        return None;
    }
    if o0a == o3a {
        return None;
    }
    Some((o0a, o3a))
}

/// Find the first removable line, in the same bottom-up order as the old
/// sequential greedy loop, whose deletion preserves deterministic divergence.
/// Multiple candidate tests run concurrently, each worker owning one CUDA
/// context on its assigned GPU.
fn find_greedy_removal(
    ptx: &str,
    input: &[u8],
    candidates: &[usize],
    gpus: &[i32],
    workers_per_gpu: usize,
    no_progress_timeout: Duration,
) -> Option<(usize, String)> {
    if candidates.is_empty() {
        return None;
    }

    let ordered: Arc<Vec<usize>> = Arc::new(candidates.iter().rev().copied().collect());
    let lines: Arc<Vec<String>> = Arc::new(ptx.lines().map(str::to_string).collect());
    let input: Arc<Vec<u8>> = Arc::new(input.to_vec());
    let next = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<(usize, bool)>();

    let mut handles = Vec::new();
    for &gpu in gpus {
        for w in 0..workers_per_gpu {
            let ordered = Arc::clone(&ordered);
            let lines = Arc::clone(&lines);
            let input = Arc::clone(&input);
            let next = Arc::clone(&next);
            let stop = Arc::clone(&stop);
            let tx = tx.clone();
            handles.push(thread::spawn(move || {
                let cuda = match Cuda::init(gpu) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("reduce worker gpu {gpu} #{w}: Cuda::init: {e:#}");
                        return;
                    }
                };
                let bufs = match cuda.alloc_buffers(input.len(), output_len()) {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("reduce worker gpu {gpu} #{w}: alloc_buffers: {e:#}");
                        return;
                    }
                };
                loop {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let pos = next.fetch_add(1, Ordering::Relaxed);
                    if pos >= ordered.len() {
                        return;
                    }
                    let remove_idx = ordered[pos];
                    let candidate = candidate_without_line(&lines, remove_idx);
                    let ok = diverges_deterministically(&cuda, &bufs, &candidate, &input).is_some();
                    if tx.send((pos, ok)).is_err() {
                        return;
                    }
                }
            }));
        }
    }
    drop(tx);

    let mut verdicts = vec![None; ordered.len()];
    let mut frontier = 0usize;
    let mut accepted = None;
    loop {
        let (pos, ok) = match rx.recv_timeout(no_progress_timeout) {
            Ok(v) => v,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                eprintln!(
                    "no reducer candidate finished for {:?}; exiting instead of hanging",
                    no_progress_timeout,
                );
                std::process::exit(124);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        verdicts[pos] = Some(ok);
        while frontier < verdicts.len() && verdicts[frontier] == Some(false) {
            frontier += 1;
        }
        if frontier < verdicts.len() && verdicts[frontier] == Some(true) {
            accepted = Some(frontier);
            stop.store(true, Ordering::Relaxed);
            break;
        }
    }

    stop.store(true, Ordering::Relaxed);
    for h in handles {
        let _ = h.join();
    }

    let pos = accepted?;
    let remove_idx = ordered[pos];
    Some((remove_idx, candidate_without_line(&lines, remove_idx)))
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
            .ok_or_else(|| anyhow!("usage: fuzzx-diff-reduce <div-dir>"))?,
    );
    let mut ptx = std::fs::read_to_string(dir.join("program.ptx"))
        .with_context(|| format!("reading program.ptx in {}", dir.display()))?;
    let input = std::fs::read(dir.join("input.bin")).context("input.bin")?;

    let gpus = parse_gpus()?;
    let workers_per_gpu = workers_per_gpu(gpus.len())?;
    let no_progress_timeout = no_progress_timeout()?;
    let cuda = Cuda::init(gpus[0]).with_context(|| format!("Cuda::init gpu={}", gpus[0]))?;
    let bufs = cuda.alloc_buffers(input.len(), output_len())?;
    let start_lines = ptx.lines().count();
    let start_body = removable_indices(&ptx)?.len();

    if diverges_deterministically(&cuda, &bufs, &ptx, &input).is_none() {
        bail!("starting PTX does not deterministically diverge — nothing to reduce");
    }
    eprintln!(
        "starting at {start_lines} total lines ({start_body} removable candidates); \
         gpus={gpus:?} workers_per_gpu={workers_per_gpu} total_workers={}",
        gpus.len() * workers_per_gpu,
    );

    let t0 = Instant::now();
    let mut total_removed = 0usize;
    loop {
        let candidates = removable_indices(&ptx)?;
        let lines: Vec<String> = ptx.lines().map(str::to_string).collect();
        // Bottom-up: removing later lines is less likely to cascade into
        // use-before-def in code that hasn't run yet. The helper runs those
        // trials in parallel but accepts the same first viable deletion this
        // greedy loop would have accepted sequentially.
        let Some((i, candidate)) = find_greedy_removal(
            &ptx,
            &input,
            &candidates,
            &gpus,
            workers_per_gpu,
            no_progress_timeout,
        ) else {
            break;
        };
        eprintln!(
            "  removed line {i:3} ({} → {} lines): {}",
            lines.len(),
            lines.len() - 1,
            lines[i].trim(),
        );
        ptx = candidate;
        total_removed += 1;
        std::fs::write(dir.join("reduced.ptx"), &ptx)?;
    }

    let end_lines = ptx.lines().count();
    let elapsed = t0.elapsed().as_secs_f64();
    eprintln!(
        "reduced {start_lines} → {end_lines} lines ({} removed) in {:.1}s",
        total_removed, elapsed,
    );

    std::fs::write(dir.join("reduced.ptx"), &ptx)?;
    if let Some((o0, o3)) = diverges_deterministically(&cuda, &bufs, &ptx, &input) {
        std::fs::write(dir.join("reduced_o0.bin"), &o0)?;
        std::fs::write(dir.join("reduced_o3.bin"), &o3)?;
        eprintln!(
            "saved reduced.ptx, reduced_o0.bin, reduced_o3.bin in {}",
            dir.display()
        );
    } else {
        bail!("reduced PTX no longer diverges (bug in reducer?)");
    }
    Ok(())
}

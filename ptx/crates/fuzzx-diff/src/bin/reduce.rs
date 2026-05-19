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

fn max_batch_size() -> Result<usize> {
    Ok(env_usize("REDUCE_MAX_BATCH")?.unwrap_or(64).max(1))
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

fn candidate_without_lines(lines: &[String], remove_indices: &[usize]) -> String {
    let mut remove = vec![false; lines.len()];
    for &i in remove_indices {
        remove[i] = true;
    }

    let mut out = String::with_capacity(lines.iter().map(|l| l.len() + 1).sum());
    for (i, line) in lines.iter().enumerate() {
        if !remove[i] {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn line_mentions_pred(line: &str, pred: &str) -> bool {
    line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '%'))
        .any(|token| token == pred)
}

fn setp_output_pred(trimmed_line: &str) -> Option<String> {
    if !trimmed_line.starts_with("setp.") {
        return None;
    }
    trimmed_line
        .split_whitespace()
        .nth(1)
        .map(|token| token.trim_end_matches(',').to_string())
        .filter(|token| token.starts_with("%p"))
}

fn line_mentions_body_wide_scratch(line: &str) -> bool {
    line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '%'))
        .any(|token| matches!(token, "%rd6" | "%rd7" | "%rd8" | "%rd9"))
}

fn line_mentions_b16_scratch(line: &str) -> bool {
    line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '%'))
        .any(|token| {
            token.strip_prefix("%h").is_some_and(|suffix| {
                !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit())
            })
        })
}

fn line_mentions_float_scratch(line: &str) -> bool {
    line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '%'))
        .any(|token| {
            token.strip_prefix("%f").is_some_and(|suffix| {
                !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit())
            }) || token.strip_prefix("%fd").is_some_and(|suffix| {
                !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit())
            })
        })
}

fn line_mentions_token(line: &str, needle: &str) -> bool {
    line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '%'))
        .any(|token| token == needle)
}

fn is_branch_line(line: &str) -> bool {
    let mut tokens = line.split_whitespace();
    let first = tokens.next();
    let op = if first.is_some_and(|token| token.starts_with('@')) {
        tokens.next()
    } else {
        first
    };
    op == Some("bra")
}

fn is_loop_counter_decrement(line: &str) -> bool {
    let line = line.trim_end_matches(';');
    let Some(rest) = line.strip_prefix("sub.u32") else {
        return false;
    };
    let operands: Vec<_> = rest.split(',').map(str::trim).collect();
    operands.len() == 3 && operands[0] == operands[1] && operands[2] == "1"
}

fn is_output_store(line: &str) -> bool {
    line.starts_with("st.global.u32")
}

fn declared_b32_scratch_reg(lines: &[&str]) -> Option<String> {
    lines.iter().find_map(|line| {
        let trimmed = line.trim();
        let prefix = ".reg .b32";
        let r_pos = trimmed.strip_prefix(prefix)?.find("%r<")? + prefix.len();
        let start = r_pos + "%r<".len();
        let end = trimmed[start..].find('>')? + start;
        let total = trimmed[start..end].parse::<u32>().ok()?;
        total.checked_sub(1).map(|reg| format!("%r{reg}"))
    })
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

fn candidate_diverges_on_gpu(gpu: i32, ptx: &str, input: &[u8]) -> Result<bool> {
    let cuda = Cuda::init(gpu).with_context(|| format!("Cuda::init gpu={gpu}"))?;
    let bufs = cuda.alloc_buffers(input.len(), output_len())?;
    let ok = diverges_deterministically(&cuda, &bufs, ptx, input).is_some();
    drop(bufs);
    drop(cuda);
    Cuda::init(gpu).with_context(|| format!("post-candidate Cuda::init gpu={gpu}"))?;
    Ok(ok)
}

/// Find the first bottom-up chunk whose combined deletion preserves
/// deterministic divergence. This is less minimal per iteration than the
/// greedy single-line pass, but much faster through long runs of dead code.
fn find_chunk_removal(
    ptx: &str,
    input: &[u8],
    candidates: &[usize],
    chunk_size: usize,
    gpus: &[i32],
    workers_per_gpu: usize,
    no_progress_timeout: Duration,
) -> Option<(Vec<usize>, String)> {
    if chunk_size <= 1 || candidates.len() < chunk_size {
        return None;
    }

    let ordered: Vec<usize> = candidates.iter().rev().copied().collect();
    let chunks: Arc<Vec<Vec<usize>>> = Arc::new(
        ordered
            .chunks(chunk_size)
            .filter(|chunk| chunk.len() == chunk_size)
            .map(|chunk| chunk.to_vec())
            .collect(),
    );
    if chunks.is_empty() {
        return None;
    }

    let lines: Arc<Vec<String>> = Arc::new(ptx.lines().map(str::to_string).collect());
    let input: Arc<Vec<u8>> = Arc::new(input.to_vec());
    let next = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<(usize, bool)>();

    let mut handles = Vec::new();
    for &gpu in gpus {
        for w in 0..workers_per_gpu {
            let chunks = Arc::clone(&chunks);
            let lines = Arc::clone(&lines);
            let input = Arc::clone(&input);
            let next = Arc::clone(&next);
            let stop = Arc::clone(&stop);
            let tx = tx.clone();
            handles.push(thread::spawn(move || loop {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                let pos = next.fetch_add(1, Ordering::Relaxed);
                if pos >= chunks.len() {
                    return;
                }
                let candidate = candidate_without_lines(&lines, &chunks[pos]);
                let ok = match candidate_diverges_on_gpu(gpu, &candidate, &input) {
                    Ok(ok) => ok,
                    Err(e) => {
                        eprintln!("reduce batch worker gpu {gpu} #{w}: candidate {pos}: {e:#}");
                        false
                    }
                };
                if tx.send((pos, ok)).is_err() {
                    return;
                }
            }));
        }
    }
    drop(tx);

    let mut verdicts = vec![None; chunks.len()];
    let mut frontier = 0usize;
    let mut accepted = None;
    loop {
        let (pos, ok) = match rx.recv_timeout(no_progress_timeout) {
            Ok(v) => v,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                eprintln!(
                    "no reducer batch candidate finished for {:?}; exiting instead of hanging",
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
    let remove_indices = chunks[pos].clone();
    let candidate = candidate_without_lines(&lines, &remove_indices);
    Some((remove_indices, candidate))
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
            handles.push(thread::spawn(move || loop {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                let pos = next.fetch_add(1, Ordering::Relaxed);
                if pos >= ordered.len() {
                    return;
                }
                let remove_idx = ordered[pos];
                let candidate = candidate_without_line(&lines, remove_idx);
                let ok = match candidate_diverges_on_gpu(gpu, &candidate, &input) {
                    Ok(ok) => ok,
                    Err(e) => {
                        eprintln!("reduce worker gpu {gpu} #{w}: candidate {pos}: {e:#}");
                        false
                    }
                };
                if tx.send((pos, ok)).is_err() {
                    return;
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
/// Sacrosanct: prologue (everything through the generated ABI/register
/// initialization), epilogue address arithmetic, labels, structural lines,
/// `ret;`, braces.
fn removable_indices(ptx: &str) -> Result<Vec<usize>> {
    let lines: Vec<&str> = ptx.lines().collect();
    let b32_scratch = declared_b32_scratch_reg(&lines);
    let prologue_end = if let Some(i) = lines.iter().position(|l| {
        let t = l.trim();
        t.starts_with("bra") && t.contains("block_0;")
    }) {
        i
    } else {
        lines
            .iter()
            .enumerate()
            .skip_while(|(_, l)| !l.trim().starts_with("ld.global.u32"))
            .find_map(|(i, l)| (i > 0 && l.trim().is_empty()).then_some(i))
            .ok_or_else(|| {
                anyhow!(
                    "could not locate `bra block_0;` or structured register-init end to mark end of prologue"
                )
            })?
    };
    let exit_start = lines
        .iter()
        .position(|l| l.trim() == "exit:")
        .or_else(|| {
            let first_store = lines.iter().position(|l| is_output_store(l.trim()))?;
            lines[..first_store]
                .iter()
                .rposition(|l| l.trim().starts_with("cvta.to.global.u64"))
        })
        .ok_or_else(|| anyhow!("could not locate `exit:` label or epilogue start"))?;
    let output_stores: Vec<usize> = (exit_start..lines.len())
        .filter(|&i| is_output_store(lines[i].trim()))
        .collect();
    if output_stores.is_empty() {
        bail!("could not locate output store");
    }
    let keep_output_store = output_stores[0];

    let mut out = Vec::new();
    // Body: between prologue_end (exclusive) and exit_start (exclusive).
    for i in (prologue_end + 1)..exit_start {
        let t = lines[i].trim();
        if t.is_empty() || t.ends_with(':') {
            continue;
        }
        // Structured-control branches and loop-counter decrements are control
        // skeleton, not reducer payload. Removing them can produce non-
        // terminating kernels that wedge the reducer's validation launch.
        if is_branch_line(t) || is_loop_counter_decrement(t) {
            continue;
        }
        // Removing a still-used predicate definition can leave an undefined
        // branch or selp predicate while still producing deterministic-looking
        // output. Unused predicate definitions are normal reducer clutter.
        if setp_output_pred(t).is_some_and(|pred| {
            lines
                .iter()
                .enumerate()
                .any(|(j, line)| j != i && line_mentions_pred(line, &pred))
        }) {
            continue;
        }
        // The generator uses `%rd6`/`%rd7` as a scratch pair for 64-bit body
        // instructions, then extracts the low half with `mov.b64`. Removing
        // only part of that sequence can leave an undefined `%rd6` read that
        // still looks deterministic on one machine.
        if line_mentions_body_wide_scratch(t) {
            continue;
        }
        // Likewise, 16-bit scalar/subword-wide instructions flow through
        // `%h*` scratch registers. Keep those chains intact instead of
        // accepting reductions that read undefined half registers.
        if line_mentions_b16_scratch(t) {
            continue;
        }
        // Floating-point operations also use scratch registers heavily.
        // Removing only a producer or consumer can leave undefined `%f*` or
        // `%fd*` reads that still pass deterministic validation.
        if line_mentions_float_scratch(t) {
            continue;
        }
        // The highest declared `%r` is the generator's scratch register for
        // register-count shifts and the high half of `mov.b64` extraction.
        // Removing either its defining mask or its use independently can leave
        // an undefined shift count.
        if b32_scratch
            .as_deref()
            .is_some_and(|reg| line_mentions_token(t, reg))
        {
            continue;
        }
        out.push(i);
    }
    // Epilogue: store lines only; address arithmetic and `ret;` stay put.
    // Keep one output store so later reducer scans still have an observation
    // point and do not accept "divergences" caused only by scratch memory.
    for i in output_stores {
        if i != keep_output_store {
            out.push(i);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{
        declared_b32_scratch_reg, is_branch_line, is_loop_counter_decrement, is_output_store,
        line_mentions_b16_scratch, line_mentions_body_wide_scratch, line_mentions_float_scratch,
        line_mentions_pred, removable_indices,
    };

    #[test]
    fn predicate_matching_does_not_confuse_prefixes() {
        assert!(line_mentions_pred("@%p1 bra label;", "%p1"));
        assert!(!line_mentions_pred("@%p10 bra label;", "%p1"));
    }

    #[test]
    fn wide_scratch_matching_does_not_confuse_prefixes() {
        assert!(line_mentions_body_wide_scratch(
            "    mov.b64       {%r16, %r33}, %rd6;"
        ));
        assert!(line_mentions_body_wide_scratch(
            "    xor.b64       %rd6, %rd6, %rd7;"
        ));
        assert!(line_mentions_body_wide_scratch(
            "    add.s64       %rd8, %rd8, %rd9;"
        ));
        assert!(!line_mentions_body_wide_scratch(
            "    mov.b64       {%r16, %r33}, %rd60;"
        ));
    }

    #[test]
    fn b16_scratch_matching_does_not_confuse_prefixes() {
        assert!(line_mentions_b16_scratch("    cvt.u16.u32   %h0, %r10;"));
        assert!(line_mentions_b16_scratch(
            "    @!%p18 mul.wide.u16 %r0, %h0, %h1;"
        ));
        assert!(!line_mentions_b16_scratch(
            "    add.u32       %r0, %r10, 16;"
        ));
        assert!(!line_mentions_b16_scratch(
            "    add.u32       %r0, %hfoo, 16;"
        ));
    }

    #[test]
    fn float_scratch_matching_does_not_confuse_prefixes() {
        assert!(line_mentions_float_scratch("    cvt.rn.f32.u32 %f0, %r17;"));
        assert!(line_mentions_float_scratch(
            "    setp.lt.and.f64 %p0, %fd0, %fd1, %p1;"
        ));
        assert!(!line_mentions_float_scratch(
            "    add.u32       %r0, %foo, 16;"
        ));
        assert!(!line_mentions_float_scratch(
            "    add.u32       %r0, %fbar, 16;"
        ));
    }

    #[test]
    fn finds_declared_b32_scratch_reg() {
        let lines = [
            ".reg .pred %p<3>;",
            ".reg .b32   %r<34>;",
            ".reg .b64 %rd<8>;",
        ];
        assert_eq!(declared_b32_scratch_reg(&lines).as_deref(), Some("%r33"));
    }

    #[test]
    fn control_skeleton_matching_catches_branches_and_loop_decrements() {
        assert!(is_branch_line("bra             exit;"));
        assert!(is_branch_line("@%p0 bra   structured_loop_done;"));
        assert!(!is_branch_line("@%p0 add.u32 %r1, %r2, %r3;"));
        assert!(is_loop_counter_decrement("sub.u32         %r9, %r9, 1;"));
        assert!(!is_loop_counter_decrement("sub.u32         %r9, %r8, 1;"));
        assert!(!is_loop_counter_decrement("sub.s32         %r9, %r9, 1;"));
        assert!(is_output_store("st.global.u32   [%rd4 + 0], %r5;"));
        assert!(!is_output_store("st.global.wt.u32 [%rd8 + 0], %r5;"));
    }

    #[test]
    fn structured_epilogue_without_exit_label_is_reducible() {
        let ptx = r#".version 8.8
.target sm_103
.address_size 64

.visible .entry fuzz_kernel()
{
    .reg .pred  %p<3>;
    .reg .b32   %r<8>;
    .reg .b64   %rd<6>;

    ld.global.u32   %r1, [%rd0];

    setp.eq.u32   %p0, %r1, %r2;
    @%p0 bra      keep;
    setp.ne.u32   %p1, %r3, %r4;
keep:
    add.u32       %r5, %r5, %r6;
    and.b32       %r7, %r1, 31;
    shl.b32       %r5, %r5, %r7;
    cvt.u64.u32   %rd6, %r1;
    cvt.u64.u32   %rd7, %r2;
    xor.b64       %rd6, %rd6, %rd7;
    mov.b64       {%r5, %r6}, %rd6;

    cvta.to.global.u64 %rd4, %rd1;
    mul.wide.u32    %rd5, %r7, 16;
    add.s64         %rd4, %rd4, %rd5;
    st.global.u32   [%rd4 + 0], %r5;
    ret;
}"#;
        let lines: Vec<_> = ptx.lines().collect();
        let removable = removable_indices(ptx).unwrap();

        assert!(removable
            .iter()
            .any(|&i| lines[i].trim().starts_with("setp.ne.u32")));
        assert!(!removable
            .iter()
            .any(|&i| lines[i].trim().starts_with("setp.eq.u32")));
        assert!(removable
            .iter()
            .any(|&i| lines[i].trim().starts_with("add.u32")));
        assert!(!removable.iter().any(|&i| lines[i].trim().contains("%r7")));
        assert!(!removable.iter().any(|&i| lines[i].trim().contains("%rd6")));
        assert!(!removable.iter().any(|&i| lines[i].trim().contains("%rd7")));
        assert!(!removable
            .iter()
            .any(|&i| lines[i].trim().starts_with("@%p0 bra")));
        assert!(!removable
            .iter()
            .any(|&i| lines[i].trim().starts_with("cvta.to.global.u64")));
    }

    #[test]
    fn keeps_one_epilogue_output_store() {
        let ptx = r#".version 8.8
.target sm_103
.address_size 64

.visible .entry fuzz_kernel()
{
    .reg .b32   %r<8>;
    .reg .b64   %rd<6>;

    ld.global.u32   %r1, [%rd0];
    add.u32       %r5, %r5, %r6;
    bra             exit;

exit:
    cvta.to.global.u64 %rd4, %rd1;
    mul.wide.u32    %rd5, %r7, 16;
    add.s64         %rd4, %rd4, %rd5;
    st.global.u32   [%rd4 + 0], %r5;
    st.global.u32   [%rd4 + 4], %r6;
    st.global.u32   [%rd4 + 8], %r7;
    ret;
}"#;
        let lines: Vec<_> = ptx.lines().collect();
        let removable = removable_indices(ptx).unwrap();
        let removable_output_stores = removable
            .iter()
            .filter(|&&i| lines[i].trim().starts_with("st.global.u32"))
            .count();

        assert_eq!(removable_output_stores, 2);
        assert!(!removable
            .iter()
            .any(|&i| lines[i].trim() == "st.global.u32   [%rd4 + 0], %r5;"));
    }
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
    let max_batch_size = max_batch_size()?;
    let start_lines = ptx.lines().count();
    let start_body = removable_indices(&ptx)?.len();

    let starting_diverges = {
        let cuda = Cuda::init(gpus[0]).with_context(|| format!("Cuda::init gpu={}", gpus[0]))?;
        let bufs = cuda.alloc_buffers(input.len(), output_len())?;
        diverges_deterministically(&cuda, &bufs, &ptx, &input).is_some()
    };
    Cuda::init(gpus[0])
        .with_context(|| format!("post-starting-candidate Cuda::init gpu={}", gpus[0]))?;
    if !starting_diverges {
        bail!("starting PTX does not deterministically diverge — nothing to reduce");
    }
    eprintln!(
        "starting at {start_lines} total lines ({start_body} removable candidates); \
         gpus={gpus:?} workers_per_gpu={workers_per_gpu} total_workers={} max_batch_size={max_batch_size}",
        gpus.len() * workers_per_gpu,
    );

    let t0 = Instant::now();
    let mut total_removed = 0usize;
    loop {
        let candidates = removable_indices(&ptx)?;
        let lines: Vec<String> = ptx.lines().map(str::to_string).collect();

        let largest_chunk = max_batch_size.min(candidates.len());
        let mut chunk_size = 1usize;
        while chunk_size.saturating_mul(2) <= largest_chunk {
            chunk_size *= 2;
        }
        let mut batch_removed = false;
        while chunk_size >= 2 {
            if let Some((remove_indices, candidate)) = find_chunk_removal(
                &ptx,
                &input,
                &candidates,
                chunk_size,
                &gpus,
                workers_per_gpu,
                no_progress_timeout,
            ) {
                let old_len = lines.len();
                let new_len = old_len - remove_indices.len();
                let first = remove_indices.iter().min().copied().unwrap_or(0);
                let last = remove_indices.iter().max().copied().unwrap_or(first);
                eprintln!(
                    "  removed batch {} lines ({} → {} lines): old lines {}..{}",
                    remove_indices.len(),
                    old_len,
                    new_len,
                    first,
                    last,
                );
                ptx = candidate;
                total_removed += remove_indices.len();
                std::fs::write(dir.join("reduced.ptx"), &ptx)?;
                batch_removed = true;
                break;
            }
            chunk_size /= 2;
        }
        if batch_removed {
            continue;
        }

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
    let cuda = Cuda::init(gpus[0]).with_context(|| format!("Cuda::init gpu={}", gpus[0]))?;
    let bufs = cuda.alloc_buffers(input.len(), output_len())?;
    let final_result = diverges_deterministically(&cuda, &bufs, &ptx, &input);
    drop(bufs);
    drop(cuda);
    Cuda::init(gpus[0])
        .with_context(|| format!("post-final-candidate Cuda::init gpu={}", gpus[0]))?;
    if let Some((o0, o3)) = final_result {
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

//! Parallel candidate sweeper for manual reduction.
//!
//! Given a base PTX file, an input.bin, and a list of "line-deletion"
//! candidates, distribute the candidates across N workers (one CUDA context
//! per worker, pinned to a GPU, reusable input/output buffers) and report
//! per-candidate whether the resulting PTX still deterministically diverges.
//!
//! Usage:
//!   divsweep <base.ptx> <input.bin> [candidates.txt]
//!
//! candidates.txt: one candidate per line, comma-separated 1-based line
//! numbers to remove from base.ptx (e.g. `34` or `34,40,45`). If absent,
//! defaults to "try removing each non-empty, non-label body line individually"
//! (body = lines after `bra block_0;`, up to but not including `exit:`).
//!
//! Output: one line per candidate: `<candidate-spec>: <verdict>`
//! Verdicts: DIVERGES, MATCHES, COMPILE_FAIL, LAUNCH_FAIL, NONDETERM, HANG.
//!
//! Each candidate is run with a wall-clock hang threshold; if `-O0` or `-O3`
//! launch takes longer than DIV_HANG_SECS (default 4), the worker rebuilds its
//! CUDA context (sacrificing throughput for that worker, but recovering the
//! GPU for everyone else).
//!
//! Env vars:
//!   DIV_GPUS              default: all visible devices
//!   DIV_WORKERS_PER_GPU   default: 16
//!   DIV_HANG_SECS         default: 4

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context as _, Result};
use ptx_fuzz_exec::{compile, Cuda, CudaBuffers};
use ptx_fuzz_execgen::{input_len, output_len, KERNEL_NAME, N_THREADS, TARGET_ARCH};

#[derive(Clone)]
struct Candidate {
    /// 1-based line numbers (into the base PTX) to remove.
    remove: Vec<usize>,
    /// Display string for reporting (the original candidates.txt line).
    spec: String,
}

#[derive(Debug, Clone)]
enum Verdict {
    Diverges { n_tids: usize },
    Matches,
    CompileFail(String),
    LaunchFail(String),
    NonDeterm(&'static str), // "O0" or "O3"
}

fn apply(base_lines: &[String], remove: &[usize]) -> String {
    use std::collections::HashSet;
    let drop: HashSet<usize> = remove.iter().copied().collect();
    let mut out = String::with_capacity(base_lines.iter().map(|l| l.len() + 1).sum());
    for (i, line) in base_lines.iter().enumerate() {
        if !drop.contains(&(i + 1)) {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn evaluate(cuda: &Cuda, bufs: &CudaBuffers, arch: &str, ptx: &str, input: &[u8]) -> Verdict {
    let launch = |cubin: &[u8]| -> std::result::Result<Vec<u8>, String> {
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
        .map_err(|e| format!("{e:#}"))
    };
    let o0_cubin = match compile(ptx, &[arch, "-O0"]) {
        Ok(c) => c,
        Err(e) => return Verdict::CompileFail(format!("-O0: {e:#}")),
    };
    let o0a = match launch(&o0_cubin) {
        Ok(b) => b,
        Err(e) => return classify_err(&e, "-O0"),
    };
    let o0b = match launch(&o0_cubin) {
        Ok(b) => b,
        Err(e) => return classify_err(&e, "-O0"),
    };
    if o0a != o0b {
        return Verdict::NonDeterm("O0");
    }
    let o3_cubin = match compile(ptx, &[arch, "-O3"]) {
        Ok(c) => c,
        Err(e) => return Verdict::CompileFail(format!("-O3: {e:#}")),
    };
    let o3a = match launch(&o3_cubin) {
        Ok(b) => b,
        Err(e) => return classify_err(&e, "-O3"),
    };
    let o3b = match launch(&o3_cubin) {
        Ok(b) => b,
        Err(e) => return classify_err(&e, "-O3"),
    };
    if o3a != o3b {
        return Verdict::NonDeterm("O3");
    }
    if o0a == o3a {
        return Verdict::Matches;
    }
    let n_tids = o0a
        .chunks(16)
        .zip(o3a.chunks(16))
        .filter(|(a, b)| a != b)
        .count();
    Verdict::Diverges { n_tids }
}

fn classify_err(msg: &str, opt: &str) -> Verdict {
    if msg.contains("ptxas") {
        Verdict::CompileFail(format!("{opt}: {msg}"))
    } else {
        Verdict::LaunchFail(format!("{opt}: {msg}"))
    }
}

fn verdict_str(v: &Verdict) -> String {
    match v {
        Verdict::Diverges { n_tids } => format!("DIVERGES ({n_tids} tids)"),
        Verdict::Matches => "MATCHES".to_string(),
        Verdict::CompileFail(m) => format!("COMPILE_FAIL {m}"),
        Verdict::LaunchFail(m) => format!("LAUNCH_FAIL {m}"),
        Verdict::NonDeterm(s) => format!("NONDETERM {s}"),
    }
}

fn parse_gpus() -> Result<Vec<i32>> {
    match std::env::var("DIV_GPUS") {
        Ok(s) => s
            .split(',')
            .map(|t| {
                t.trim()
                    .parse::<i32>()
                    .map_err(|e| anyhow!("DIV_GPUS entry {t:?}: {e}"))
            })
            .collect(),
        Err(_) => {
            let n = Cuda::device_count().context("Cuda::device_count")?;
            if n <= 0 {
                anyhow::bail!("no CUDA devices visible");
            }
            Ok((0..n).collect())
        }
    }
}

fn body_lines(base_lines: &[String]) -> Vec<usize> {
    let prologue_end = base_lines
        .iter()
        .position(|l| {
            let t = l.trim();
            t.starts_with("bra") && t.contains("block_0;")
        })
        .unwrap_or(0);
    let exit_start = base_lines
        .iter()
        .position(|l| l.trim() == "exit:")
        .unwrap_or(base_lines.len());
    let mut out = Vec::new();
    for i in (prologue_end + 1)..exit_start {
        let t = base_lines[i].trim();
        if t.is_empty() || t.ends_with(':') || t.ends_with('}') {
            continue;
        }
        out.push(i + 1); // 1-based
    }
    out
}

fn main() -> Result<()> {
    if std::env::var_os("TMPDIR").is_none() {
        let shm = std::path::Path::new("/dev/shm");
        if shm.is_dir() {
            std::env::set_var("TMPDIR", shm);
        }
    }

    let base_path = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or_else(|| anyhow!("usage: divsweep <base.ptx> <input.bin> [candidates.txt]"))?,
    );
    let input_path = PathBuf::from(
        std::env::args()
            .nth(2)
            .ok_or_else(|| anyhow!("usage: divsweep <base.ptx> <input.bin> [candidates.txt]"))?,
    );
    let cand_path = std::env::args().nth(3).map(PathBuf::from);

    let base =
        std::fs::read_to_string(&base_path).with_context(|| base_path.display().to_string())?;
    let input = std::fs::read(&input_path).with_context(|| input_path.display().to_string())?;
    let base_lines: Vec<String> = base.lines().map(str::to_string).collect();

    let candidates: Vec<Candidate> = match cand_path {
        Some(p) => {
            let txt = std::fs::read_to_string(&p).with_context(|| p.display().to_string())?;
            txt.lines()
                .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
                .map(|line| {
                    let remove = line
                        .split(',')
                        .map(|t| t.trim().parse::<usize>().expect("line number"))
                        .collect();
                    Candidate {
                        remove,
                        spec: line.trim().to_string(),
                    }
                })
                .collect()
        }
        None => body_lines(&base_lines)
            .into_iter()
            .map(|n| Candidate {
                remove: vec![n],
                spec: n.to_string(),
            })
            .collect(),
    };

    let gpus = parse_gpus()?;
    let workers_per_gpu: usize = std::env::var("DIV_WORKERS_PER_GPU")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(16);
    let hang_secs: u64 = std::env::var("DIV_HANG_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);

    eprintln!(
        "divsweep: base={} candidates={} gpus={:?} workers_per_gpu={} (total={})",
        base_path.display(),
        candidates.len(),
        gpus,
        workers_per_gpu,
        gpus.len() * workers_per_gpu,
    );

    let next = Arc::new(AtomicUsize::new(0));
    let results: Arc<Mutex<Vec<Option<Verdict>>>> =
        Arc::new(Mutex::new(vec![None; candidates.len()]));
    let arch = format!("-arch={TARGET_ARCH}");

    // Two layers of in-flight tracking:
    //   in_flight: workers that have grabbed an index but not yet finished.
    //   done:      candidates that have produced a verdict.
    // A hung kernel (infinite-loop PTX) blocks one worker forever; we want
    // the OTHER workers to drain the queue and the main thread to give up
    // after a grace period rather than waiting on the hung worker.
    let in_flight = Arc::new(AtomicUsize::new(0));
    let done_count = Arc::new(AtomicUsize::new(0));

    for &gpu in &gpus {
        for w in 0..workers_per_gpu {
            let candidates = candidates.clone();
            let base_lines = base_lines.clone();
            let input = input.clone();
            let next = Arc::clone(&next);
            let results = Arc::clone(&results);
            let in_flight = Arc::clone(&in_flight);
            let done_count = Arc::clone(&done_count);
            let arch = arch.clone();
            thread::spawn(move || {
                let cuda = match Cuda::init(gpu) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("gpu {gpu} worker {w}: Cuda::init: {e:#}");
                        return;
                    }
                };
                let bufs = match cuda.alloc_buffers(input_len(), output_len()) {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("gpu {gpu} worker {w}: alloc_buffers: {e:#}");
                        return;
                    }
                };
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= candidates.len() {
                        return;
                    }
                    in_flight.fetch_add(1, Ordering::Relaxed);
                    let cand = &candidates[i];
                    let ptx = apply(&base_lines, &cand.remove);
                    let v = evaluate(&cuda, &bufs, &arch, &ptx, &input);
                    results.lock().unwrap()[i] = Some(v);
                    in_flight.fetch_sub(1, Ordering::Relaxed);
                    done_count.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
    }

    // Main thread: wait until queue is empty AND in-flight is empty, OR until
    // queue has been empty for a grace period (in which case we abandon the
    // hung workers). After abandoning, print whatever we have; any None entry
    // is reported as HANG.
    let grace = Duration::from_secs(8);
    let no_progress_timeout = Duration::from_secs(hang_secs.max(1));
    let n = candidates.len();
    let mut queue_emptied_at: Option<std::time::Instant> = None;
    let mut last_done = 0usize;
    let mut last_progress_at = std::time::Instant::now();
    let assigned_at_stop = loop {
        thread::sleep(Duration::from_millis(250));
        let assigned = next.load(Ordering::Relaxed);
        let in_f = in_flight.load(Ordering::Relaxed);
        let done = done_count.load(Ordering::Relaxed);
        if done != last_done {
            last_done = done;
            last_progress_at = std::time::Instant::now();
        }
        if done >= n {
            break assigned;
        }
        if in_f > 0 && last_progress_at.elapsed() >= no_progress_timeout {
            eprintln!(
                "divsweep: no candidate finished for {:?}; abandoning {} in-flight workers",
                no_progress_timeout, in_f,
            );
            break assigned;
        }
        if assigned >= n {
            // Queue is empty. Start (or check) grace period.
            let started = *queue_emptied_at.get_or_insert_with(std::time::Instant::now);
            if started.elapsed() >= grace {
                eprintln!(
                    "divsweep: giving up on {} in-flight workers after {:?} grace",
                    in_f, grace,
                );
                break assigned;
            }
        }
    };

    let results = results.lock().unwrap();
    let mut diverges = 0usize;
    let mut hangs = 0usize;
    for (i, cand) in candidates.iter().enumerate() {
        match results[i].as_ref() {
            Some(v) => {
                let mark = match v {
                    Verdict::Diverges { .. } => {
                        diverges += 1;
                        "+"
                    }
                    _ => " ",
                };
                println!("{mark} {}: {}", cand.spec, verdict_str(v));
            }
            None if i < assigned_at_stop => {
                hangs += 1;
                println!("H {}: HANG", cand.spec);
            }
            None => {
                println!("? {}: NOT_RUN", cand.spec);
            }
        }
    }
    eprintln!(
        "summary: {diverges} diverged, {hangs} hung, {} other/not-run out of {n}",
        n - diverges - hangs,
    );
    // Force-exit so any hung worker thread doesn't keep us alive.
    std::process::exit(0);
}

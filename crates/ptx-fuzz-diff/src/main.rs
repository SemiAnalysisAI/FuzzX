//! Inner-loop differential fuzzer for ptxas.
//!
//! For each iteration:
//!   1. Derive a byte buffer from `starting_seed + iter`.
//!   2. Generate a PTX kernel.
//!   3. Compile + launch at both `-O0` and `-O3` on the GPU.
//!   4. If outputs differ (or compile/launch is asymmetric), save the
//!      reproducer under `<out_dir>/div-<timestamp>-<seed>/`.
//!
//! Configured via env vars (env over args for parity with the AFL scripts):
//!   DIV_OUT_DIR          default: `divergences`
//!   DIV_STARTING_SEED    default: nanos since epoch
//!   DIV_MAX_ITERS        default: unlimited
//!   DIV_PRINT_EVERY_SECS default: 5
//!   DIV_PROGRAM_BYTES    default: 4096

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use ptx_fuzz_exec::{differential, Cuda, DiffOutcome};
use ptx_fuzz_execgen::{
    bytes_from_seed, generate_from_bytes, input_for_seed, output_len, KERNEL_NAME, N_THREADS,
    TARGET_ARCH,
};

struct Config {
    out_dir: PathBuf,
    starting_seed: u64,
    max_iters: Option<u64>,
    print_every: Duration,
    program_bytes: usize,
}

impl Config {
    fn from_env() -> Result<Self> {
        fn env<T: std::str::FromStr>(key: &str) -> Result<Option<T>>
        where
            T::Err: std::fmt::Display,
        {
            match std::env::var(key) {
                Ok(v) => v
                    .parse()
                    .map(Some)
                    .map_err(|e| anyhow::anyhow!("env {key}={v:?} parse error: {e}")),
                Err(_) => Ok(None),
            }
        }
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        Ok(Config {
            out_dir: env::<String>("DIV_OUT_DIR")?
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("divergences")),
            starting_seed: env("DIV_STARTING_SEED")?.unwrap_or(nanos),
            max_iters: env("DIV_MAX_ITERS")?,
            print_every: Duration::from_secs(env("DIV_PRINT_EVERY_SECS")?.unwrap_or(5)),
            program_bytes: env("DIV_PROGRAM_BYTES")?.unwrap_or(4096),
        })
    }
}

fn save_divergence(
    out_dir: &Path,
    seed: u64,
    bytes: &[u8],
    ptx: &str,
    input: &[u8],
    outcome: &DiffOutcome,
) -> Result<PathBuf> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dir = out_dir.join(format!("div-{ts}-{seed:016x}"));
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating {}", dir.display()))?;
    std::fs::write(dir.join("seed.bin"), bytes)?;
    std::fs::write(dir.join("program.ptx"), ptx)?;
    std::fs::write(dir.join("input.bin"), input)?;
    match &outcome.o0 {
        Ok(b) => std::fs::write(dir.join("output_o0.bin"), b)?,
        Err(e) => std::fs::write(dir.join("output_o0.err"), format!("{e:#}"))?,
    }
    match &outcome.o3 {
        Ok(b) => std::fs::write(dir.join("output_o3.bin"), b)?,
        Err(e) => std::fs::write(dir.join("output_o3.err"), format!("{e:#}"))?,
    }
    let summary = format!(
        "seed: {seed}\nseed_hex: {seed:016x}\no0_ok: {}\no3_ok: {}\nverdict: {}\n",
        outcome.o0.is_ok(),
        outcome.o3.is_ok(),
        match (&outcome.o0, &outcome.o3) {
            (Ok(a), Ok(b)) if a == b => "MATCH",
            (Ok(_), Ok(_)) => "OUTPUT_MISMATCH",
            (Ok(_), Err(_)) => "O3_FAILED_O0_OK",
            (Err(_), Ok(_)) => "O0_FAILED_O3_OK",
            (Err(_), Err(_)) => "BOTH_FAILED",
        }
    );
    std::fs::write(dir.join("summary.txt"), summary)?;
    Ok(dir)
}

fn main() -> Result<()> {
    let cfg = Config::from_env()?;
    std::fs::create_dir_all(&cfg.out_dir)?;
    let cuda = Cuda::init(0).context("Cuda::init")?;
    let arch = format!("-arch={TARGET_ARCH}");

    eprintln!(
        "ptx-fuzz-diff: starting_seed=0x{:016x} out={} program_bytes={} max_iters={}",
        cfg.starting_seed,
        cfg.out_dir.display(),
        cfg.program_bytes,
        cfg.max_iters
            .map(|n| n.to_string())
            .unwrap_or_else(|| "∞".to_string()),
    );

    let start = Instant::now();
    let mut last_print = start;
    let mut iter: u64 = 0;
    let mut divergences: u64 = 0;
    let mut both_failed: u64 = 0;
    let mut skipped: u64 = 0;

    loop {
        if let Some(max) = cfg.max_iters {
            if iter >= max {
                break;
            }
        }

        let seed = cfg.starting_seed.wrapping_add(iter);
        let bytes = bytes_from_seed(seed, cfg.program_bytes);
        let ptx = match generate_from_bytes(&bytes) {
            Ok(p) => p,
            Err(_) => {
                skipped += 1;
                iter += 1;
                continue;
            }
        };
        let input = input_for_seed(seed);
        let outcome = differential(
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

        if outcome.diverged() {
            divergences += 1;
            let dir = save_divergence(&cfg.out_dir, seed, &bytes, &ptx, &input, &outcome)?;
            eprintln!("DIVERGENCE seed=0x{seed:016x} saved={}", dir.display());
        } else if !outcome.matches() {
            both_failed += 1;
        }

        iter += 1;

        if last_print.elapsed() >= cfg.print_every {
            let elapsed = start.elapsed().as_secs_f64();
            let rate = iter as f64 / elapsed.max(1e-6);
            eprintln!(
                "iter {iter}  {rate:.1} iter/s  divergences {divergences}  both_failed {both_failed}  skipped {skipped}  elapsed {elapsed:.0}s",
            );
            last_print = Instant::now();
        }
    }

    let elapsed = start.elapsed().as_secs_f64();
    eprintln!(
        "done. iter={iter} divergences={divergences} both_failed={both_failed} skipped={skipped} elapsed={elapsed:.1}s rate={:.1} iter/s",
        iter as f64 / elapsed.max(1e-6),
    );
    Ok(())
}

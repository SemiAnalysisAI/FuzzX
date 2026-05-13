# ptx-fuzz

Coverage-guided fuzzer for NVIDIA's `ptxas` PTX assembler. Driven by
[AFL++](https://github.com/AFLplusplus/AFLplusplus) in qemu_mode, with
a tiny custom-mutator shared library that turns AFL's mutated byte
inputs into PTX source text just before they reach the target.

## Architecture

```
afl-fuzz ─ loads ─► libptx_fuzz_mutator.so       (just bytes → PTX text)
   │                          ▲
   │ writes mutated bytes ────┘ afl_custom_post_process
   │ to @@ input file
   ▼
afl-qemu-trace ──► ptxas @@   (TCG blocks instrumented; edge counts
                               written to AFL's shmem coverage map)
```

AFL's corpus stores **raw bytes**; AFL's built-in mutators happily
splice and flip those. The mutator hook only runs at the last
moment — it never lets PTX text get fed back into AFL's byte-level
mutators (which would destroy syntactic structure).

See [DESIGN.md](DESIGN.md) for the trade-offs.

## Layout

| Path                              | Purpose                                                                      |
| --------------------------------- | ---------------------------------------------------------------------------- |
| `crates/ptx-fuzz-gen`             | Pure function `&[u8] -> String` that emits PTX source from arbitrary bytes.  |
| `crates/ptx-fuzz-mutator`         | `cdylib` exporting AFL++'s custom-mutator C ABI; wraps `ptx-fuzz-gen`.       |
| `crates/ptx-fuzz-repro`           | CLI to re-apply the mutator to a saved corpus/crash file (for triage).      |
| `crates/fake-ptxas`               | Stand-in target binary with seeded crashes, for local testing.              |
| `seeds/`                          | Initial corpus — six PTX fragments that each assemble cleanly on their own. |
| `scripts/run-fuzz.sh`             | Single-core `afl-fuzz -Q` invocation with all the right env vars wired up.   |
| `scripts/run-fuzz-multi.sh`       | N-worker variant (one `-M` master + N-1 `-S` secondaries) for parallel runs. |
| `scripts/triage.sh`               | Group saved crashes across all workers by (exit code, stderr signature).     |

## Running on Linux (real fuzzing)

### Known-good AFL++ build

| Component           | Known good                                                               |
| ------------------- | ------------------------------------------------------------------------ |
| `afl-fuzz`          | AFL++ **4.41a** (built from the `dev` branch, May 2026).                 |
| `afl-qemu-trace`    | Built from the same checkout via `qemu_mode/build_qemu_support.sh`.      |

Two AFL bugs to know about — both worked around, neither root-caused.
DESIGN.md has the full story; the relevant facts for running:

  - **AFL++ 4.40c does not work.** Workers segfault inside `libc.so.6`
    (`malloc/arena.c`, `malloc.c`, `memmove-vec-unaligned-erms.S`) at
    ~3 deaths/minute even with one worker. *Fix: upgrade.* Some
    commit between 4.40c and 4.41a fixes it as a side effect; we
    didn't bisect.
  - **AFL++ 4.41a workers crash at ≥50 cores unless the trim stage
    is disabled.** Every crash lands at libc `0xac52e`
    (`malloc.c:4267`, `_int_malloc` walking a freelist) — looks like
    a UAF or off-by-one in AFL's trim code. *Workaround:
    `AFL_DISABLE_TRIM=1`.* Trim is a cosmetic optimization for
    shrinking accepted corpus entries; not load-bearing for our use
    case.

### Known-good environment

`scripts/run-fuzz.sh` and `scripts/run-fuzz-multi.sh` set the
following automatically — listed here so it's clear what we depend
on and why, in case you're driving AFL by hand:

| Env var                       | Value | Reason                                                            |
| ----------------------------- | ----- | ----------------------------------------------------------------- |
| `AFL_CUSTOM_MUTATOR_LIBRARY`  | path  | Load our byte→PTX mutator.                                        |
| `AFL_SKIP_BIN_CHECK`          | `1`   | ptxas wasn't compiled with AFL instrumentation.                   |
| `AFL_FRAMESHIFT_DISABLE`      | `1`   | FrameShift corrupts the heap when combined with a post_process mutator (4.40c+). |
| `AFL_DISABLE_TRIM`            | `1`   | Avoids the 4.41a `_int_malloc` crash at high worker counts.       |
| `AFL_NO_UI`                   | `1`   | Multi-core workers log to files, not a curses TTY.                |

### Building AFL++

```bash
# On the Linux host:
git clone https://github.com/AFLplusplus/AFLplusplus
cd AFLplusplus
git checkout origin/dev     # 4.41a or newer
make source-only            # afl-fuzz; skips nyx_mode's nightly-Rust build
cd qemu_mode && ./build_qemu_support.sh
cd .. && sudo make install  # afl-fuzz, afl-qemu-trace on $PATH
afl-fuzz --version          # should say 'afl-fuzz++4.41a' (or newer)
```

Then from this repo:

```bash
# Single-core. ~30 execs/sec through QEMU.
PTXAS=$(which ptxas) scripts/run-fuzz.sh

# Multi-core. Default: min(nproc, 16) workers. AFL++ syncs the
# corpus across all of them, so this is close to linear speedup.
PTXAS=$(which ptxas) CORES=16 scripts/run-fuzz-multi.sh

# Optional: stop the multi-core run after a fixed budget.
PTXAS=$(which ptxas) CORES=16 RUNTIME=600 scripts/run-fuzz-multi.sh
```

For multi-core runs, crashes land in `output/<worker>/crashes/`
(where `<worker>` is `main` or `secNN`). `afl-whatsup -s output` gives
a cross-worker summary.

To reproduce a saved crash:

```bash
cargo build --release -p ptx-fuzz-repro
target/release/ptx-fuzz-repro output/main/crashes/id:000000,... --run
```

To triage everything saved across all workers in one pass:

```bash
PTXAS=$(which ptxas) scripts/triage.sh output
cat output/triage/summary.txt
# Per-group: output/triage/group-NN/{example.ptx,example.stderr,members.txt}
```

## Local sanity checks (macOS or Linux)

You can't run qemu_mode on macOS, but you can build everything and
exercise the mutator + the seeded-crash target:

```bash
cargo build --workspace
cargo test --workspace

# Confirm the mutator produces valid-looking PTX from random bytes.
head -c 50 /dev/urandom > /tmp/rand.bin
cargo run -q -p ptx-fuzz-repro -- /tmp/rand.bin

# Confirm fake-ptxas crashes on the seeded pattern.
printf '@!' > /tmp/crash.ptx && target/debug/fake-ptxas /tmp/crash.ptx
# expect: exit 139 (SIGSEGV)
```

## Environment variables (user-facing)

These are the knobs you'd normally set when running. The internal
`AFL_*` flags that the scripts export are documented in
"Known-good environment" above.

| Variable                       | Meaning                                                              |
| ------------------------------ | -------------------------------------------------------------------- |
| `PTXAS`                        | Target binary path. Default: `ptxas` from `$PATH`.                   |
| `SEEDS_DIR` / `OUT_DIR`        | Override AFL seed-corpus and output dirs.                            |
| `TIMEOUT_MS`                   | Per-iteration hang limit (default 5000 ms).                          |
| `CORES`                        | Multi-core workers (`run-fuzz-multi.sh` only). Default: min(nproc, 16). 100 verified stable on a 224-core box. |
| `RUNTIME`                      | Multi-core only. Seconds to run before killing all workers.          |

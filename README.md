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

The Linux box `jlebar-dev` has CUDA but probably not AFL++ yet. Install:

```bash
# On the Linux host:
git clone https://github.com/AFLplusplus/AFLplusplus
cd AFLplusplus
make distrib                 # builds afl-fuzz and the LLVM compilers
cd qemu_mode && ./build_qemu_support.sh
cd .. && sudo make install   # puts afl-fuzz, afl-qemu-trace on $PATH
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

## Environment variables

| Variable                       | Meaning                                                              |
| ------------------------------ | -------------------------------------------------------------------- |
| `PTXAS`                        | Target binary path. Default: `ptxas` from `$PATH`.                   |
| `SEEDS_DIR` / `OUT_DIR`        | Override AFL seed-corpus and output dirs in `scripts/run-fuzz.sh`.   |
| `TIMEOUT_MS`                   | Per-iteration hang limit (default 5000 ms).                          |
| `CORES`                        | Multi-core workers (`run-fuzz-multi.sh` only). Default: min(nproc, 16). |
| `RUNTIME`                      | Multi-core only. Seconds to run before killing all workers.          |
| `AFL_CUSTOM_MUTATOR_LIBRARY`   | Set automatically by the scripts; AFL++ dlopens this.                |
| `AFL_SKIP_BIN_CHECK`           | Set automatically; tells AFL ptxas wasn't AFL-instrumented at compile time. |
| `AFL_FRAMESHIFT_DISABLE`       | Set automatically. AFL++ 4.41a's FrameShift stage corrupts heap when combined with a post_process mutator. |

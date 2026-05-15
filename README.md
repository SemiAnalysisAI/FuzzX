# FuzzX

FuzzX finds NVIDIA `ptxas` miscompiles by generating PTX programs, compiling
each one at multiple optimization levels, running the resulting cubins on CUDA,
and saving cases where the outputs disagree.

The fuzzer is intentionally undirected. It walks a deterministic seed stream
instead of using coverage feedback.

## Requirements

| Component | Notes |
| --- | --- |
| Rust | Uses the toolchain in `rust-toolchain.toml`. |
| CUDA driver + `libcuda` | Required for fuzzing, verification, and reduction. |
| `ptxas` | Set `PTXAS=/path/to/ptxas` for reproducible runs. |
| NVIDIA GPU matching `TARGET_ARCH` | `fuzzx-execgen` currently defaults to `sm_103`. |

## Layout

| Path | Purpose |
| --- | --- |
| `crates/fuzzx-execgen` | PTX kernel generator for differential testing. |
| `crates/fuzzx-exec` | `ptxas` compiler wrapper plus CUDA launch/diff helpers. |
| `crates/fuzzx-diff` | Differential fuzzer plus show/verify/reduce helpers. |
| `known-miscompiles/` | Reduced or standalone reproducers for confirmed findings. |
| `scripts/check-gen.sh` | Generator acceptance-rate smoke test against `ptxas`. |

## Running

Build the tools:

```bash
cargo build --release -p fuzzx-diff
```

Run a differential sweep:

```bash
target/release/fuzzx-diff \
  --ptxas /usr/local/cuda/bin/ptxas \
  --max-iters 100000
```

Divergences are saved under `DIV_OUT_DIR` (default: `divergences/`) as
directories containing `seed.bin`, `program.ptx`, `input.bin`, `output_o0.*`,
`output_o3.*`, and `summary.txt`.

Useful follow-up commands:

```bash
target/release/fuzzx-diff-show divergences/div-...
target/release/fuzzx-diff-verify divergences/div-...
target/release/fuzzx-diff-reduce divergences/div-...
target/release/fuzzx-diff-test divergences/div-.../program.ptx divergences/div-.../input.bin
target/release/fuzzx-diff-inspect-outputs divergences/div-.../program.ptx divergences/div-.../input.bin
```

Check how often generated PTX assembles:

```bash
PTXAS=/usr/local/cuda/bin/ptxas scripts/check-gen.sh 200
```

## Configuration

`fuzzx-diff` accepts kebab-case CLI flags for the run-control and generator
settings below; `target/release/fuzzx-diff --help` lists the full set. The
same settings can still be supplied as environment variables, which is useful
for long-running scripted sweeps. Boolean environment variables accept `1`,
`true`, `yes`, or `on` for true, and `0`, `false`, `no`, or `off` for false.

### Shared

| Variable | Default | Meaning |
| --- | --- | --- |
| `PTXAS` | `/usr/local/cuda/bin/ptxas`, then `$HOME/bin/ptxas`, then `ptxas` | Target `ptxas` binary. Set this explicitly for reproducible runs. |
| `TMPDIR` | Caller value; some tools use `/dev/shm` when unset and available. | Temporary directory for PTX/cubin files. |

### Run Control

| Variable | Default | Meaning |
| --- | --- | --- |
| `DIV_OUT_DIR` | `divergences` | Directory for saved divergence bundles. |
| `DIV_STARTING_SEED` | nanoseconds since epoch | First seed in the deterministic seed stream. |
| `DIV_MAX_ITERS` | unlimited | Stop after this many generated candidates. |
| `DIV_PRINT_EVERY_SECS` | `5` | Progress-report interval. |
| `DIV_PROGRAM_BYTES` | `4096` | Bytes derived from each seed and consumed by the generator. |
| `DIV_GPUS` | all visible CUDA devices | Comma-separated CUDA device ordinals, for example `0,1,2`. |
| `DIV_WORKERS_PER_GPU` | `16` | Worker threads per selected GPU. |

### Generator Shape

| Variable | Default | Meaning |
| --- | --- | --- |
| `DIV_STRUCTURED_CONTROL_FLOW` | `false` | Use structured single-entry if/loop generation instead of arbitrary CFG generation. |
| `DIV_MIN_BLOCKS` / `DIV_MAX_BLOCKS` | `1` / `10` | Block-count bounds. |
| `DIV_MIN_INSTS_PER_BLOCK` / `DIV_MAX_INSTS_PER_BLOCK` | `1` / `6` | Instruction-count bounds per block. |
| `DIV_WORKING_REGS` | `8` | Number of working `u32` registers. |
| `DIV_MAX_LOOP_ITERS` | `16` | Maximum generated loop-trip count. |
| `DIV_MAX_IMMEDIATE` | `32` | Maximum ordinary immediate value. |
| `DIV_MAX_STRUCTURED_DEPTH` | `3` | Maximum nesting depth for structured control flow. |

### Generator Feature Toggles

All variables in this table default to `false`; setting one to true suppresses
that feature.

| Variable | Suppresses |
| --- | --- |
| `DIV_DISABLE_STRUCTURED_LOOPS` | Counted-loop shapes in structured mode. |
| `DIV_DISABLE_ARBITRARY_LOOPS` | Backedge loop terminators in arbitrary CFG mode. |
| `DIV_DISABLE_LOP3` | `lop3.b32`. |
| `DIV_DISABLE_MINMAX` | `min.u32`, `max.u32`, `min.s32`, `max.s32`. |
| `DIV_DISABLE_SUB` | Random `sub.u32` ALU instructions. |
| `DIV_DISABLE_MULHI` | `mul.hi.u32` and `mul.hi.s32`. |
| `DIV_DISABLE_SIGNED_MULHI` | `mul.hi.s32` only. |
| `DIV_DISABLE_BITWISE_BINOPS` | `and.b32`, `or.b32`, `xor.b32`. |
| `DIV_DISABLE_PRMT` | `prmt.b32`. |
| `DIV_DISABLE_NOT` | `not.b32` and xor-by-`0xffffffff` forms. |
| `DIV_DISABLE_CLZ` | `clz.b32`. |
| `DIV_DISABLE_CNOT` | `cnot.b32`. |
| `DIV_DISABLE_ABS` | `abs.s32`. |
| `DIV_DISABLE_SIGNED_CMP` | Signed predicate comparisons. |
| `DIV_DISABLE_SIGNED_DIVREM` | `div.s32` and `rem.s32`. |
| `DIV_DISABLE_FUNNEL` | `shf.l.wrap.b32` and `shf.r.wrap.b32`. |
| `DIV_DISABLE_NEG` | `neg.s32`. |
| `DIV_DISABLE_SHL` | `shl.b32`. |
| `DIV_DISABLE_SIGNED_SHR` | `shr.s32`. |
| `DIV_DISABLE_BFIND` | `bfind.u32` and `bfind.shiftamt.u32`. |
| `DIV_DISABLE_BFI` | `bfi.b32`. |
| `DIV_DISABLE_BMSK` | `bmsk.clamp.b32`. |
| `DIV_DISABLE_MAD24` | `mad24.lo.u32` and `mad24.hi.u32`. |
| `DIV_DISABLE_MUL24` | `mul24.{lo,hi}.{u32,s32}`. |
| `DIV_DISABLE_MUL_WIDE` | `mul.wide.{u32,s32}`. |
| `DIV_DISABLE_WIDE_INT` | 64-bit scratch-register ALU generation. |
| `DIV_DISABLE_ADDC` | `add.cc.u32` / `addc.u32` pairs. |
| `DIV_DISABLE_SUBC` | `sub.cc.u32` / `subc.u32` pairs. |
| `DIV_DISABLE_I32_BOUNDARY_IMMS` | Immediate `0x7fffffff` / `0x80000000` generation. |
| `DIV_DISABLE_DP2A` | `dp2a.{lo,hi}.u32.u32`. |
| `DIV_DISABLE_SET` | `set.{cmp}.u32.{u32,s32}`. |
| `DIV_DISABLE_S32_SLCT` | `slct.s32.s32`. |
| `DIV_DISABLE_VIDEO` | PTX video instructions. |
| `DIV_DISABLE_VSUB4` | `vsub4.u32.u32.u32`. |

### Reduction And Sweeping

| Variable | Default | Meaning |
| --- | --- | --- |
| `REDUCE_GPUS` | `DIV_GPUS`, then all visible devices | CUDA devices used by `fuzzx-diff-reduce`. |
| `REDUCE_WORKERS_PER_GPU` | `DIV_WORKERS_PER_GPU`, then host-core based default capped at `16` | Reducer worker count per GPU. |
| `REDUCE_NO_PROGRESS_SECS` | `120` | Reducer timeout when no candidate completes. |
| `DIV_HANG_SECS` | `4` | `fuzzx-diff-sweep` no-progress threshold before reporting hangs. |

## License

FuzzX is licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).

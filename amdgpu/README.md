# FuzzX AMDGPU

*Human-written content*

This is a vibe-coded fuzzer for the AMDGPU path in LLVM.

We test the full LLVM IR -> AMDGPU assembly compilation path, although in
practice most of the bugs we're finding are in the AMDGPU-specific parts of the
compiler.

The idea is to:
 - generate programs that have defined semantics (no UB or poison),
 - compile them with -O0 and -O2,
 - ensure that -O0 and -O2 have the same result, and
 - compare that result to that of a trusted interpreter.

In most of the reproducers we've found, -O0 gives the wrong result and -O2
gives the correct result.  My untested hypothesis is that we could find
reproducers for most of these bugs at -O2 as well, it's just that LLVM is good
at simplifying code, and simpler code is less likely to hit a backend bug.

I initially used LLVM HEAD as the primary fuzzing target, but many of the bugs
I found didn't reproduce in the latest ROCm release.  (IOW HEAD has regressions
compared to the release.)  Seeing this, I figured I should be fuzzing the
release instead.  After m038, AMD asked us to switch active fuzzing back to
HEAD builds; the current upstream LLVM HEAD column has
llvm/llvm-project#196418, llvm/llvm-project#198412,
llvm/llvm-project#198491, llvm/llvm-project#198508, and
llvm/llvm-project#198556 applied locally (the last three are AMD-provided
fixes for the m001, m003/m005/m012/m014, and m026-m029 bug classes; 198556
supersedes the older 198373 and 198419 bitop3 fixes that previous builds
carried).  In any case, the table of results below shows which versions
reproduce which bugs.

Everything below this line is AI-generated.  You probably only care about the
"bugs generated" table.  Good luck.

-----------

This directory contains the AMDGPU fuzzer work area.  It is intentionally
separate from the PTX / `ptxas` fuzzer in [`../ptx/`](../ptx/).

The AMDGPU fuzzer is the directed C++ libFuzzer target in `fuzzer/`. Its only
input format is an LLVM bitcode module containing an AMDGPU kernel named
`fuzz_kernel`. For each input module, the fuzzer compiles the kernel through
`-O0` and `-O2` LLVM pipelines, links both code objects into one HSACO, runs
both kernels through HIP, and compares device output. Set
`FUZZX_USE_LLVM_INTERPRETER_ORACLE=1` to also run an LLVM-interpreter oracle
for modules that do not use AMDGPU-specific intrinsics beyond workgroup and
workitem IDs and do not use FP types. Pure LLVM integer bit-counting and
byte-swap intrinsics are allowed in oracle-compatible modules. The interpreter
clone scalarizes vector integer intrinsics and lowers safe LLVM integer min/max,
saturation, absolute value, funnel-shift, bit-reverse, and overflow intrinsics
to plain IR before execution. Oracle findings include the expected value in
`mismatch.txt`.
Set `FUZZX_REQUIRE_LLVM_INTERPRETER_ORACLE=1` for an oracle-focused campaign
where mutation and crossover keep only interpreter-compatible modules.

The custom mutator and crossover operate on LLVM IR rather than on raw bytes.
They currently build a conservative, defined subset of integer IR: no `undef`,
no explicit poison values, no `nuw` / `nsw` / `exact`, no `inbounds`, no
integer division except nonzero-denominator `udiv` / `urem`, only masked or
constant shift amounts, and only the fixed skeleton input load/output store.
Coverage includes scalar `i32` integer arithmetic, bitwise ops,
compares/selects, masked dynamic shifts, rare signed division/remainder by proven-positive divisors,
standalone `i8` / `i16` scalar subexpressions, `i64` subexpressions truncated
to `i32`, `<2 x i32>` / `<4 x i32>` vector subexpressions including fixed
`shufflevector` masks, and narrow `<4/8 x i8>` / `<4/8 x i16>` vector
subexpressions reduced back to `i32`,
scalar and vector forms of LLVM bit/min/max/saturation/absolute intrinsics,
narrow scalar funnel shifts and unsigned division/remainder by proven-nonzero
denominators, explicit `i1` boolean subexpressions reduced back to `i32`,
pure-IR unsigned min/max and saturating add/sub select idioms, and
pure-IR masked funnel-shift/rotate idioms, pure-IR signed add/sub overflow
select idioms, pure-IR predicate-mask blend/sign idioms, and pure-IR bitfield
extract/insert idioms, pure-IR byte/word pack-unpack idioms, pure-IR widening
multiply-high/low idioms, pure-IR byte dot-product chain idioms, pure-IR
bit-count/bit-twiddle idioms, pure-IR
average/absolute-difference idioms, and pure-IR lane clamp/saturating-pack
idioms, pure-IR vector shuffle/horizontal-reduction idioms, pure-IR
carry/borrow-chain idioms, pure-IR dynamic byte extraction/permutation idioms,
pure-IR compare-rank/mask idioms, pure-IR ternary bit-logic idioms, pure-IR
64-bit pair arithmetic idioms, and pure-IR byte-prefix/permutation idioms,
pure-IR overflow-chain idioms, pure-IR select lookup-table idioms, and pure-IR
nibble reduction idioms, pure-IR SWAR bit tricks, pure-IR byte compare/mask
idioms, pure-IR limb multiply/add idioms, pure-IR select-network idioms,
pure-IR vector compare/mask pack idioms, pure-IR byte Horner-mix idioms,
pure-IR bit ballot/matrix-pack idioms, pure-IR halfword compare/pack idioms,
pure-IR nibble table-lookup idioms, pure-IR bit deposit/extract idioms,
pure-IR i64 byte-permutation idioms, and pure-IR narrow-vector min/max idioms,
pure-IR byte-lane select idioms, pure-IR halfword dot-accumulate idioms,
pure-IR rotate/mask cascade idioms, and pure-IR vector byte gather idioms,
pure-IR byte-prefix compare and byte median/range idioms, pure-IR i64
cross-lane fold idioms, pure-IR vector pairwise byte arithmetic idioms,
pure-IR byte permute-control idioms, pure-IR bit-run mask idioms, pure-IR i64
multiply-fold idioms, pure-IR halfword blend-network idioms, pure-IR byte
ternary-blend idioms, pure-IR halfword prefix-sum idioms, pure-IR i64
rotate-add idioms, pure-IR vector compare bitmask idioms, pure-IR byte carry
propagation idioms, pure-IR bit-slice boolean idioms, pure-IR vector
splat/blend idioms, pure-IR i64 compare/pack idioms, pure-IR nibble
carry-chain idioms, pure-IR halfword saturating-difference idioms, pure-IR i64
bitfield-mix idioms, pure-IR vector lane mix/pack idioms, pure-IR byte
saturating pack idioms, pure-IR halfword multiply-high idioms, pure-IR i64
prefix-fold idioms, and pure-IR vector byte rotate/pack idioms, alongside
LLVM bit, min/max, saturation, absolute-value, funnel-shift, and integer
overflow intrinsics. It also emits a small AMDGPU-specific pure
integer-intrinsic subset covering BFE, SAD/MSAD, `lerp`, 24-bit multiply,
packed SAD/MQSAD, `alignbyte`, signed first-bit-high, `mbcnt`, `perm`,
explicit `bitop3`, `readfirstlane`, wave reductions, and integer dot-product
operations, plus bounded AMDGPU FP/packing intrinsics such as
`fmed3`, `frexp`, `fract`, `class`, and packed FP/int conversions. Known
`sudot*` and `fma.legacy` instruction-selection crashes are gated off by
default. It also emits a finite
scalar FP subset by masking
inputs to small nonnegative integers, converting with `uitofp`, using exact
`fadd` / `fmul` / nonzero-denominator `fdiv` / `fcmp` / `select` shapes, and
converting back with in-range `fptoui`; a signed variant uses small
sign-extended integers, `sitofp`, `fadd` / `fsub` / `fmul` /
nonzero-denominator `fdiv`, and in-range `fptosi`. It also emits finite scalar
`half` and `<2/4 x half>` / `<2/4 x float>` vector FP subexpressions reduced
back to `i32`. The mutator can
also wrap the current result in structured two-way
branches, wider multi-way switches, branch/PHI cascades, and deeper bounded CFG
subgraphs with `i32` phi joins. Those subgraphs can nest more diamonds, switches,
cascades, and small counted loops with optional guarded early exits. The mutator
also generates top-level counted loops with bounded constant or dynamically
masked trip counts whose bodies can contain nested diamonds, switches, cascades,
and inner loops. A dedicated loop-nest mutation wraps an inner counted loop and
optional tail CFG inside an outer bounded loop. A complex-CFG mutation chains
several nested subgraphs before the final store, so a single corpus entry can
contain multiple high-fanout joins and loop nests instead of just one wrapper
around the result. Some generated loops carry two independent `i32` accumulator
phis, combine them after the loop, take a guarded early exit from the loop
body through an exit phi, or switch from the loop body to multiple distinct exit
values before one joined exit phi, so corpus entries exercise both expression
simplification and CFG and loop transforms. CFG arms include the same scalar
integer, bit, boolean, narrowing, saturating, funnel-shift, finite-FP, and vector
expression families as the linear mutator. Scalar and CFG expressions can also
mix in extra `i32` global input loads from `in[seed % n]`; these loads are only
emitted inside the existing `idx < n` guard and are bounded by the module
validator.
Corpus files can be inspected directly with `opt -S corpus-entry -o -`.

## Requirements

| Component | Notes |
| --- | --- |
| ROCm LLVM | Defaults to `/opt/rocm-7.1.1/lib/llvm/bin/clang-20`, `lld`, and `llvm-objdump`; override with `CLANG`, `LLD`, and `LLVM_OBJDUMP`. |
| HIP | `hipcc` is used to build the module runner. |
| AMDGPU | Defaults to `gfx950`; override with `--mcpu`. |

## Run

Build the current upstream-HEAD LLVM fuzzing toolchain and run the directed C++
GPU differential fuzzer:

```bash
scripts/build_instrumented_llvm.sh
scripts/build_directed_fuzzer.sh
HIP_DEVICE=0 scripts/run_directed_fuzzer.sh -runs=100 -max_len=131072
```

Run one directed fuzzer process per GPU:

```bash
scripts/run_directed_multigpu_fuzzer.sh -runs=1000 -max_len=131072
```

Run multiple directed fuzzer workers on each selected GPU:

```bash
WORKERS_PER_GPU=2 scripts/run_directed_multigpu_fuzzer.sh -runs=1000 -max_len=131072
```

Multi-GPU runs share one live libFuzzer corpus by default, so workers can
reload inputs discovered by other workers while keeping per-worker logs and
artifact directories. Set `FUZZX_CORPUS_MODE=isolated` to return to one
independent corpus directory per worker.
Fresh corpus directories are seeded with a valid LLVM bitcode module before
libFuzzer starts. Set `FUZZX_IMPORT_CORPUS` to one or more colon-separated
files or directories to copy an older corpus into the fresh corpus before
workers launch.

For the current upstream-HEAD campaign, run multiple workers across all GPUs:

```bash
GPUS="0 1 2 3 4 5 6 7" WORKERS_PER_GPU=12 \
  FUZZX_REQUIRE_LLVM_INTERPRETER_ORACLE=1 \
  FUZZX_IMPORT_CORPUS=/tmp/old-run/corpus/directed-gpu/shared \
  scripts/run_directed_multigpu_fuzzer.sh \
    -max_total_time=900 -max_len=131072 -rss_limit_mb=8192 -use_value_profile=1
```

With an optimized LLVM build using sanitizer coverage and no ASan, the directed
fuzzer currently reaches about 500 exec/s aggregate across 8 GPUs.
Keep the corpus, logs, artifacts, findings, and `TMPDIR` on a local filesystem;
the run scripts default these hot paths to `/tmp/fuzzx-amdgpu-$USER` through
`FUZZX_RUNTIME_ROOT`. Avoid putting them on WekaFS or another shared filesystem,
because libFuzzer produces a high rate of tiny metadata and log writes. The run
scripts also copy the fuzzer binary into the local runtime root by default
before spawning workers; set `FUZZX_LOCALIZE_FUZZER=0` to disable that. When
Weka client frontend processes reserve dedicated CPU cores, the run scripts
default `FUZZX_CPUSET=auto`, detect single-core-pinned `wekanode` processes, and
run fuzzer workers through `taskset` on the remaining CPUs. Set
`FUZZX_CPUSET=none` to disable this or `FUZZX_CPUSET=0-63` to use an explicit
CPU set.

For historical ROCm 7.2.3 release fuzzing, use the release wrapper:

```bash
scripts/run_rocm_7_2_3_release_fuzzer.sh -max_total_time=900 -max_len=131072 -rss_limit_mb=8192 -use_value_profile=1
```

That wrapper selects the ROCm 7.2.3 fuzzer build instead of the current
upstream-HEAD fuzzer build.

Candidate compiler crashes, compile/link failures, or output mismatches are
saved under `$FUZZX_RUNTIME_ROOT/findings` by default. Generated corpora and
findings are local artifacts and are ignored by git; set `FUZZX_RUNTIME_ROOT`,
`CORPUS_ROOT`, `LOG_DIR`, `ARTIFACT_ROOT`, or `FUZZX_FINDINGS_DIR` to override
the default local runtime paths.

### Known-Bug Suppression

Known bug patterns are suppressed by default so continued fuzzing does not keep
rediscovering the same issue.

| Flag | Default | Meaning |
| --- | --- | --- |
| `FUZZX_ALLOW_M016_SCALAR_FSHL=1` | unset | Re-enable scalar `llvm.fshl.i32` generation for [m015](known-miscompiles/m015-scalar-fshl-zero/NOTES.md), [m016](known-miscompiles/m016-scalar-fshl-one/NOTES.md), and [m070](known-miscompiles/m070-scalar-fshl-shift8/NOTES.md); the legacy `FUZZX_ALLOW_M015_SCALAR_FSHL_ZERO=1` flag is also accepted. |
| `FUZZX_ALLOW_M026_UMAX_XOR_AND_HIGHBIT=1` | unset | Re-enable `(umax(a, b) ^ b) & umax(a, b)` shapes for [m026](known-miscompiles/m026-shl-umax-xor-and/NOTES.md). |
| `FUZZX_ALLOW_M028_UMAX_AND_NOT=1` | unset | Re-enable `(umax((y & ~x), C) & x) & ~x` shapes for [m028](known-miscompiles/m028-umax-and-not/NOTES.md). |
| `FUZZX_ALLOW_M030_CTLZ_SHL_OR_BITOP3=1` | unset | Re-enable `or(add(shl(...), z), z)` and `or(smin(add(shl(...), z), z), z)` tails for [m030](known-miscompiles/m030-ctlz-shl-or-bitop3/NOTES.md). |
| `FUZZX_ALLOW_M031_VECTOR_OR_EXTRACT_SUB=1` | unset | Re-enable subtracting two scalar extracts from the same vector `or` for [m031](known-miscompiles/m031-vector-or-extract-sub/NOTES.md). |
| `FUZZX_ALLOW_M032_LOOP_VECTOR_SELECT=1` | unset | Re-enable loop-carried values whose backedge depends on a vector `select` for [m032](known-miscompiles/m032-loop-vector-select/NOTES.md). |
| `FUZZX_ALLOW_M035_WAVE_REDUCE_XOR=1` | unset | Re-enable `llvm.amdgcn.wave.reduce.xor` generation for [m035](known-miscompiles/m035-wave-reduce-xor-constant/NOTES.md). |
| `FUZZX_ALLOW_M036_WAVE_REDUCE_ADD=1` | unset | Re-enable `llvm.amdgcn.wave.reduce.add` generation for [m036](known-miscompiles/m036-wave-reduce-add-constant/NOTES.md). |
| `FUZZX_ALLOW_M039_SEXT_I8_HIGHBYTE=1` | unset | Re-enable `sext i8 to i32` values feeding high-byte extraction for [m039](known-miscompiles/m039-sext-i8-highbyte-pack/NOTES.md). |
| `FUZZX_ALLOW_M040_SIGNED_DIVREM24=1` | unset | Re-enable signed `sdiv` / `srem` by small odd denominators when the numerator is not known to fit signed 24-bit for [m040](known-miscompiles/m040-sdivrem24-boundary/NOTES.md). |
| `FUZZX_ALLOW_M041_ASHR_HIGHBYTE_PACK=1` | unset | Re-enable high-byte extraction from `ashr i32` values feeding byte-pack shapes for [m041](known-miscompiles/m041-ashr-highbyte-pack/NOTES.md). |
| `FUZZX_ALLOW_M045_UREM_OR_ONE=1` | unset | Re-enable `urem x, (x \| 1)` shapes for [m045](known-miscompiles/m045-urem-or-one-known24/NOTES.md). |
| `FUZZX_ALLOW_M046_V4I16_CTTZ=1` | unset | Re-enable `llvm.cttz.v4i16` shapes for [m046](known-miscompiles/m046-v4i16-cttz-funnel-loop/NOTES.md). |
| `FUZZX_ALLOW_M047_V8I8_SHL=1` | unset | Re-enable `<8 x i8>` vector `shl` shapes for [m047](known-miscompiles/m047-bytedot-v8i8-shl-loop/NOTES.md). |
| `FUZZX_ALLOW_M048_V8I8_UADD_SAT=1` | unset | Re-enable `llvm.uadd.sat.v8i8` shapes for [m048](known-miscompiles/m048-v8i8-uadd-sat-vecreduce-loop/NOTES.md). |
| `FUZZX_ALLOW_M049_VECTOR_FSHL=1` | unset | Re-enable vector `llvm.fshl` calls for [m049](known-miscompiles/m049-vector-fshl-zero/NOTES.md); the legacy `FUZZX_ALLOW_M049_VECTOR_FSHL_ZERO=1` flag is also accepted. |
| `FUZZX_ALLOW_M051_VECTOR_FSHR_LOOP=1` | unset | Re-enable vector `llvm.fshr` calls for [m051](known-miscompiles/m051-vector-fshr-divergent-loop/NOTES.md). |
| `FUZZX_ALLOW_M052_TERNARY_BLEND_SHIFT=1` | unset | Re-enable `((a ^ b) \| (b & ~(a ^ b))) & 31` shift masks for [m052](known-miscompiles/m052-ternary-blend-shift/NOTES.md). |
| `FUZZX_ALLOW_M053_BYTEDOT_HIGHBIT=1` | unset | Re-enable byte-dot result values feeding a high-bit mask for [m053](known-miscompiles/m053-bytedot-highbit/NOTES.md). |
| `FUZZX_ALLOW_M054_I64_PAIR_LOW_ADD=1` | unset | Re-enable `((zext x << 32) \| 0xffff) + zext x` pair-add shapes for [m054](known-miscompiles/m054-i64-pair-low-add/NOTES.md). |
| `FUZZX_ALLOW_M055_I64BYTEPERM_LOOP=1` | unset | Re-enable loop-carried values depending on i64 byte-permutation idioms for [m055](known-miscompiles/m055-i64byteperm-loop-readfirstlane/NOTES.md). |
| `FUZZX_ALLOW_M056_HALFDOT_BRANCH=1` | unset | Re-enable low-bit branch keys depending on halfword-dot pack values for [m056](known-miscompiles/m056-halfdot-lowbit-branch/NOTES.md). |
| `FUZZX_ALLOW_M057_ROTCASCADE_STORE=1` | unset | Re-enable final stores depending on rotate-cascade values for [m057](known-miscompiles/m057-rotcascade-store/NOTES.md). |
| `FUZZX_ALLOW_M058_NIBBLE_BYTESEL_HIGHBIT=1` | unset | Re-enable byte-lane select carry values derived from nibble-table packs for [m058](known-miscompiles/m058-nibble-bytesel-highbit/NOTES.md). |
| `FUZZX_ALLOW_M060_PACKUNPACK_BYTEDOT=1` | unset | Re-enable final stores depending on generated `packunpack` byte-dot sums for [m060](known-miscompiles/m060-packunpack-bytedot-dot4/NOTES.md). |
| `FUZZX_ALLOW_M061_OVMASKPACK_OVERFLOW=1` | unset | Re-enable final stores depending on generated `ovmaskpack` overflow/byte-pack values for [m061](known-miscompiles/m061-ovmaskpack-o0-overflow-lowering/NOTES.md). |
| `FUZZX_ALLOW_M062_BYTEHIST_BITMUX=1` | unset | Re-enable final stores depending on both generated `bytehist` and `bitmux` values for [m062](known-miscompiles/m062-bytehist-bitmux-lowbyte/NOTES.md). |
| `FUZZX_ALLOW_M063_OVERFLOW_CARRY_BITOP3=1` | unset | Re-enable final stores depending on generated `carry` values for [m063](known-miscompiles/m063-overflow-carry-bitop3/NOTES.md). |
| `FUZZX_ALLOW_M064_NIBBLECARRY_LOOP=1` | unset | Re-enable loop-carried final stores depending on generated `nibblecarry` values for [m064](known-miscompiles/m064-nibblecarry-loop-readfirstlane/NOTES.md). |
| `FUZZX_ALLOW_M065_USUB_OVERFLOW_XOR_FOLD=1` | unset | Re-enable final stores depending on generated `ovbytegather` values for [m065](known-miscompiles/m065-usub-overflow-xor-fold/NOTES.md). |
| `FUZZX_ALLOW_M066_VECI16ZEXTMUL_BITOP3_LOOP=1` | unset | Re-enable loop-carried final stores depending on generated `veci16zextmul` values for [m066](known-miscompiles/m066-veci16zextmul-bitop3-loop/NOTES.md). |
| `FUZZX_ALLOW_M067_BYTECONDSEL_SELF_AND=1` | unset | Re-enable final stores depending on generated `bytecondsel` values for [m067](known-miscompiles/m067-bytecondsel-and-i1-self/NOTES.md). |
| `FUZZX_ALLOW_M068_LOOP_VOP3FUSED_UMAXBITOP3=1` | unset | Re-enable final stores depending on generated `umaxbitop3cascade` values for [m068](known-miscompiles/m068-loop-vop3fused-umaxbitop3/NOTES.md) (shares a suppressor with m069). |
| `FUZZX_ALLOW_M069_UMAXBITOP3CASCADE_STORE=1` | unset | Same `umaxbitop3cascade` suppressor as m068; see [m069](known-miscompiles/m069-umaxbitop3cascade-store/NOTES.md). |
| `FUZZX_ALLOW_C001_SUDOT_ISEL_ICE=1` | unset | Re-enable `llvm.amdgcn.sudot4` / `llvm.amdgcn.sudot8` generation for [c001](known-miscompiles/c001-sudot-isel-ice/NOTES.md). |
| `FUZZX_ALLOW_C002_FMA_LEGACY_ISEL_ICE=1` | unset | Re-enable `llvm.amdgcn.fma.legacy` generation for [c002](known-miscompiles/c002-fma-legacy-isel-ice/NOTES.md). |

## Layout

| Path | Purpose |
| --- | --- |
| `third_party/llvm-project` | LLVM source checkout, pinned as a git submodule. |
| `patches/llvm-pr-198373.diff` | Local source-fix patch for the current HEAD campaigns; `scripts/build_instrumented_llvm.sh` applies it by default to the selected `LLVM_PROJECT_DIR`. |
| `patches/llvm-pr-196418.diff` | Local patch for unsigned `LowerDIVREM24`; `scripts/build_instrumented_llvm.sh` applies it by default to the selected `LLVM_PROJECT_DIR`. |
| `patches/llvm-pr-198412.diff` | Local patch for non-add AMDGPU dot-product add-chain matching; `scripts/build_instrumented_llvm.sh` applies it by default to the selected `LLVM_PROJECT_DIR`. |
| `patches/llvm-pr-198419.diff` | Local source-fix patch for AMDGPU `BitOp3_Op` shared-source aliasing; `scripts/build_instrumented_llvm.sh` applies it by default to the selected `LLVM_PROJECT_DIR`. |
| `scripts/build_instrumented_llvm.sh` | Helper for configuring a sanitizer-coverage LLVM source build. |
| `scripts/build_directed_fuzzer.sh` | Builds the C++ GPU differential libFuzzer target. |
| `scripts/seed_ir_corpus.sh` | Writes the initial LLVM bitcode corpus seed. |
| `scripts/run_directed_fuzzer.sh` | Runs the C++ directed fuzzer on one GPU. |
| `scripts/run_directed_multigpu_fuzzer.sh` | Runs one or more C++ directed fuzzer processes per selected GPU. |
| `scripts/run_rocm_7_2_3_release_fuzzer.sh` | Runs the C++ directed fuzzer with the ROCm 7.2.3 release build. |
| `fuzzer/` | LLVM API plus HIP differential libFuzzer target. |
| `runner/hip_module_runner.cpp` | HIP module loader used to execute generated HSACO files. |
| `known-miscompiles/` | Reduced or standalone reproducers for confirmed findings. |

## AMDGPU Bugs Found

Except where otherwise noted, these have been tested on `gfx950`.  The result
columns report the generic `known-miscompiles/run_ll_reproducer.sh`
differential test: ✅ means no mismatch was observed for that reproducer, and
❌ means the toolchain reproduces the `-O0` / `-O2` mismatch.
Confirmed compiler ICEs should be recorded here too, with the table entry
describing the crashing toolchain and phase instead of a differential result.

Tested toolchains as of 2026-05-19:

| Column | Toolchain |
| --- | --- |
| ROCm release | [ROCm 7.2.3 source tag](https://github.com/ROCm/llvm-project/releases/tag/rocm-7.2.3), commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`; also checked against the matching [ROCm 7.2.3 `rocm-llvm` package](https://repo.radeon.com/rocm/apt/7.2.3/pool/main/r/rocm-llvm/rocm-llvm_22.0.0.26084.70203-90~22.04_amd64.deb), package SHA256 `4c406e184f88949cea60869949454e5392e1cbd9480c4c87274f7b59e9f810e5`. |
| LLVM HEAD | https://github.com/llvm/llvm-project/commit/0dd29960cd6102b37651cc3f58f872652099b83b (2026-05-18) plus [llvm/llvm-project#196418](https://github.com/llvm/llvm-project/pull/196418), [llvm/llvm-project#198412](https://github.com/llvm/llvm-project/pull/198412), [llvm/llvm-project#198491](https://github.com/llvm/llvm-project/pull/198491), [llvm/llvm-project#198508](https://github.com/llvm/llvm-project/pull/198508), and [llvm/llvm-project#198556](https://github.com/llvm/llvm-project/pull/198556), built `Release` with sanitizer coverage, no ASan. |
| ROCm HEAD | https://github.com/ROCm/llvm-project/commit/a5de13684ba84db953b28e632ea304080a4318d0 (2026-05-18) plus [llvm/llvm-project#196418](https://github.com/llvm/llvm-project/pull/196418), [llvm/llvm-project#198412](https://github.com/llvm/llvm-project/pull/198412), [llvm/llvm-project#198491](https://github.com/llvm/llvm-project/pull/198491), [llvm/llvm-project#198508](https://github.com/llvm/llvm-project/pull/198508) (source-only; the patch's `.ll` test diffs do not apply against ROCm-staging baseline checks), and [llvm/llvm-project#198556](https://github.com/llvm/llvm-project/pull/198556), built with assertions, ASan, and sanitizer coverage. |

| Bug | ROCm 7.2.3 | LLVM HEAD | ROCm HEAD | Description |
| --- | --- | --- | --- | --- |
| [m001-ashr-i16-zext](known-miscompiles/m001-ashr-i16-zext/NOTES.md) | ❌ | ✅ | ✅ | `ashr i16` feeding `zext i16 to i32` is folded to a sign-extending SDWA byte select; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198491. |
| [m002-i8-clear-xor](known-miscompiles/m002-i8-clear-xor/NOTES.md) | ✅ | ✅ | ✅ | `-O0` lowers a byte-clear xor through `v_bitop3_b32` with the wrong result; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198373. |
| [m003-shl3-add-chain](known-miscompiles/m003-shl3-add-chain/NOTES.md) | ✅ | ✅ | ✅ | `-O0` scalarizes a divergent `shl3/add` chain through `v_readfirstlane_b32`; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198508. |
| [m004-vector-identity-xor](known-miscompiles/m004-vector-identity-xor/NOTES.md) | ✅ | ✅ | ✅ | `-O0` loses a lane-0 vector identity before `xor`; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198373. |
| [m005-shl1-add-chain](known-miscompiles/m005-shl1-add-chain/NOTES.md) | ✅ | ✅ | ✅ | `-O0` scalarizes a divergent `shl1/add` chain through the same class of bug as m003; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198508. |
| [m006-i8-xor-clear](known-miscompiles/m006-i8-xor-clear/NOTES.md) | ✅ | ✅ | ✅ | `-O0` lowers another adjacent `i8` narrow byte-clear xor through the wrong `v_bitop3_b32` result; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198373. |
| [m007-vector-shl-identity-xor](known-miscompiles/m007-vector-shl-identity-xor/NOTES.md) | ✅ | ✅ | ✅ | `-O0` loses a vector shift-by-zero lane-0 identity before `xor`; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198373. |
| [m008-i8-separated-clear](known-miscompiles/m008-i8-separated-clear/NOTES.md) | ✅ | ✅ | ✅ | `-O0` miscompiles an `i8` identity byte-clear xor when prior narrow ops are separated by no-op adds; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198373. |
| [m009-i16-clear-xor](known-miscompiles/m009-i16-clear-xor/NOTES.md) | ✅ | ✅ | ✅ | `-O0` miscompiles an `i16` identity low-16 clear xor through the wrong `v_bitop3_b32` result; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198373. |
| [m010-i16-sext-clear-xor](known-miscompiles/m010-i16-sext-clear-xor/NOTES.md) | ✅ | ✅ | ✅ | `-O0` miscompiles an `i16` sign-extended identity clear xor through the wrong `v_bitop3_b32` result; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198373. |
| [m011-i8-sext-clear-xor](known-miscompiles/m011-i8-sext-clear-xor/NOTES.md) | ✅ | ✅ | ✅ | `-O0` miscompiles an `i8` sign-extended masked clear xor through the wrong `v_bitop3_b32` result; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198373. |
| [m012-add-shl-ladder](known-miscompiles/m012-add-shl-ladder/NOTES.md) | ✅ | ✅ | ✅ | `-O0` scalarizes a divergent `add/shl` ladder through `v_readfirstlane_b32`; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198508. |
| [m013-private-memory-fshl](known-miscompiles/m013-private-memory-fshl/NOTES.md) | ❌ | ❌ | ❌ | `-O0` lowers fixed private-memory allocas through a dynamic scratch stack sequence that can return intermittent wrong values. |
| [m014-shl-add-ctpop](known-miscompiles/m014-shl-add-ctpop/NOTES.md) | ✅ | ✅ | ✅ | `-O0` scalarizes a four-step `shl/add` chain feeding `ctpop` through lane 0; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198508. |
| [m015-scalar-fshl-zero](known-miscompiles/m015-scalar-fshl-zero/NOTES.md) | ✅ | ❌ | ❌ | `-O0` lowers scalar `fshl.i32(x, y, 0)` through a 64-bit shift-by-`-1` sequence that returns zero. |
| [m016-scalar-fshl-one](known-miscompiles/m016-scalar-fshl-one/NOTES.md) | ✅ | ❌ | ❌ | `-O0` lowers scalar `fshl.i32(x, y, 1)` through a 64-bit shift-by-`-1` sequence that returns only bit 31. |
| [m017-vector-and-lane0-clear-xor](known-miscompiles/m017-vector-and-lane0-clear-xor/NOTES.md) | ❌ | ✅ | ✅ | ROCm 7.2.3 `-O0` drops a vector lane-0 `and`/`extractelement` clear before `xor`; LLVM HEAD and ROCm HEAD already pass. |
| [m018-two-private-memory-ops](known-miscompiles/m018-two-private-memory-ops/NOTES.md) | ❌ | ✅ | ✅ | ROCm 7.2.3 `-O0` intermittently reads stale scratch data across two private-memory sequences; LLVM HEAD and ROCm HEAD pass 50 repeated combined runs. |
| [m019-highbit-or-xor](known-miscompiles/m019-highbit-or-xor/NOTES.md) | ❌ | ✅ | ✅ | `-O0` combines a high-bit `(x \| C) ^ x` expression into `v_bitop3_b32` with the wrong truth table or operands; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198419. |
| [m020-or-xor-and](known-miscompiles/m020-or-xor-and/NOTES.md) | ❌ | ✅ | ✅ | `-O0` combines `((a \| b) ^ b) & (a \| b)` into `v_bitop3_b32` with the wrong result; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198419. |
| [m021-fshl-or-xor](known-miscompiles/m021-fshl-or-xor/NOTES.md) | ❌ | ✅ | ✅ | `-O0` combines a dynamic `(a \| b) ^ a` expression after `fshl` into `v_bitop3_b32` with the wrong result; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198419. |
| [m022-and-xor-constant](known-miscompiles/m022-and-xor-constant/NOTES.md) | ❌ | ✅ | ✅ | `-O0` combines `((x ^ C) & x)` after a dynamic `and` into `v_bitop3_b32` with the wrong low bit; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198419. |
| [m023-and-xor-identity](known-miscompiles/m023-and-xor-identity/NOTES.md) | ❌ | ✅ | ✅ | `-O0` combines `(x & y) ^ x` into `v_bitop3_b32` with the wrong identity result; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198419. |
| [m024-udiv-or-one](known-miscompiles/m024-udiv-or-one/NOTES.md) | ❌ | ✅ | ✅ | `-O0` lowers unsigned division of a sign-extended `i16` value by `x \| 1` through an imprecise float reciprocal path; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#196418. |
| [m025-urem-or-one](known-miscompiles/m025-urem-or-one/NOTES.md) | ❌ | ✅ | ✅ | `-O0` lowers unsigned remainder of a sign-extended `i16` value by `x \| 1` through the same imprecise reciprocal path; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#196418. |
| [m026-shl-umax-xor-and](known-miscompiles/m026-shl-umax-xor-and/NOTES.md) | ❌ | ❌ | ❌ | `-O2` combines a shifted `umax` high-bit extraction into `v_bitop3_b32` using the input and salt instead of their xor; llvm/llvm-project#198556 does not catch this shape. |
| [m027-xor-and-or](known-miscompiles/m027-xor-and-or/NOTES.md) | ❌ | ✅ | ✅ | `-O0` combines `(((y ^ x) & x) \| base)` into `v_bitop3_b32` with the wrong bit when `x` is `(base ^ z) & base`; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198556. |
| [m028-umax-and-not](known-miscompiles/m028-umax-and-not/NOTES.md) | ❌ | ❌ | ❌ | `-O0` combines `(umax((y & ~x), C) & x) & ~x` into `v_bitop3_b32` using the input and salt separately; llvm/llvm-project#198556 does not catch this shape. |
| [m029-fshl-select-phi](known-miscompiles/m029-fshl-select-phi/NOTES.md) | ❌ | ✅ | ✅ | `-O2` lowers a signed compare/select over `y & x`, where `x` is a complemented masked `fshl`, so the true zero arm is chosen when the signed compare is false; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198556. |
| [m030-ctlz-shl-or-bitop3](known-miscompiles/m030-ctlz-shl-or-bitop3/NOTES.md) | ❌ | ❌ | ❌ | `-O2` lowers a low-bit `or` through `v_bitop3_b32` using the unmasked `%n` value instead of `%n & 1`. |
| [m031-vector-or-extract-sub](known-miscompiles/m031-vector-or-extract-sub/NOTES.md) | ❌ | ✅ | ✅ | ROCm 7.2.3 `-O2` scalarizes a vector `or` extract/sub as `or(x, 255) - x` instead of `or(x, 255) - -1`. |
| [m032-loop-vector-select](known-miscompiles/m032-loop-vector-select/NOTES.md) | ❌ | ✅ | ✅ | ROCm 7.2.3 `-O2` kills the loop EXEC mask before storing a loop-carried value derived from a vector `select`. |
| [m033-sub-zext-bool-fp](known-miscompiles/m033-sub-zext-bool-fp/NOTES.md) | ❌ | ✅ | ✅ | `-O2` lowers `sub i32 X, zext(i1 Cond)` through `s_subb_u32` with the wrong false-case borrow before a masked FP accumulation; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198412. |
| [m034-fshl-add-workitem-product](known-miscompiles/m034-fshl-add-workitem-product/NOTES.md) | ❌ | ✅ | ✅ | `-O2` rewrites a workitem-product `fshl`/add chain as a byte dot product that returns `0xffffffff` instead of `0xc0000000` for `x == 0`; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198412. |
| [m035-wave-reduce-xor-constant](known-miscompiles/m035-wave-reduce-xor-constant/NOTES.md) | ❌ | ✅ | ✅ | ROCm 7.2.3 `-O2` folds `llvm.amdgcn.wave.reduce.xor.i32(30, 0)` to `30` instead of the even-wave XOR result `0`. |
| [m036-wave-reduce-add-constant](known-miscompiles/m036-wave-reduce-add-constant/NOTES.md) | ❌ | ✅ | ✅ | ROCm 7.2.3 `-O2` folds `llvm.amdgcn.wave.reduce.add.i32(65536, 1)` to `65536` instead of the full-wave sum `0x00400000`. |
| [m037-dot4-square-lowbit](known-miscompiles/m037-dot4-square-lowbit/NOTES.md) | ❌ | ✅ | ✅ | `-O2` lowers a byte-masked `x*x + (x*x & 1)` expression to `v_perm_b32` / `v_dot4_u32_u8` with an extra constant accumulator; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198412. |
| [m038-loop-fp-mask-xor](known-miscompiles/m038-loop-fp-mask-xor/NOTES.md) | ❌ | ✅ | ✅ | `-O2` unrolls nested xor loops and folds a masked integer-to-FP round-trip into a byte-dot sequence that adds `1023` for input zero; LLVM HEAD and ROCm HEAD pass after llvm/llvm-project#198412. |
| [m039-sext-i8-highbyte-pack](known-miscompiles/m039-sext-i8-highbyte-pack/NOTES.md) | ❌ | ❌ | ❌ | `-O2` packs bytes after an `i8` sign-extension but clears the byte lanes contributed by the sign bits. |
| [m040-sdivrem24-boundary](known-miscompiles/m040-sdivrem24-boundary/NOTES.md) | ❌ | ❌ | ❌ | `-O2` applies the signed 24-bit reciprocal division lowering when the positive numerator has bit 23 set, returning a quotient one too large. |
| [m041-ashr-highbyte-pack](known-miscompiles/m041-ashr-highbyte-pack/NOTES.md) | ❌ | ❌ | ❌ | `-O2` lowers a byte pack after `ashr i32` to `v_perm_b32` with the wrong high-byte lane. |
| [m042-or-lshr-zero-xor](known-miscompiles/m042-or-lshr-zero-xor/NOTES.md) | ✅ | ✅ | ✅ | `-O0` lowered `or x, (lshr x, 0)` where `x` is `(a ^ b) \| ((a ^ b) >> 1)` through the wrong `v_bitop3_b32`; LLVM HEAD passes after llvm/llvm-project#198373. |
| [m043-zext-i8-self-xor](known-miscompiles/m043-zext-i8-self-xor/NOTES.md) | ✅ | ✅ | ✅ | `-O0` lowered `xor x, x`, where `x` is `zext(trunc(workitem.id.x)) ^ 1`, through `v_bitop3_b32`; LLVM HEAD passes after llvm/llvm-project#198373. |
| [m044-v4i32-self-and-zero-shuffle](known-miscompiles/m044-v4i32-self-and-zero-shuffle/NOTES.md) | ✅ | ✅ | ✅ | `-O0` lowered a `<4 x i32>` `and x, x` lane ORed with a zero shuffle through `v_bitop3_b32`; LLVM HEAD passes after llvm/llvm-project#198373. |
| [m045-urem-or-one-known24](known-miscompiles/m045-urem-or-one-known24/NOTES.md) | ❌ | ❌ | ❌ | `-O2` lowers `urem x, (x \| 1)` with known 24-bit `x` to `0x00ffffff` instead of `x` when even `x` is smaller than `x \| 1`; explicit masking can make `-O0` wrong too. |
| [m046-v4i16-cttz-funnel-loop](known-miscompiles/m046-v4i16-cttz-funnel-loop/NOTES.md) | ✅ | ❌ | ❌ | `-O2` miscomputes a dynamic-trip nested loop whose body extracts a lane from `llvm.cttz.v4i16` and feeds a funnel-shift-shaped scalar expression. |
| [m047-bytedot-v8i8-shl-loop](known-miscompiles/m047-bytedot-v8i8-shl-loop/NOTES.md) | ✅ | ❌ | ❌ | `-O2` folds a byte-dot-style dynamic loop with a `<8 x i8>` vector shift to `4` for lanes where `-O0` produces smaller values. |
| [m048-v8i8-uadd-sat-vecreduce-loop](known-miscompiles/m048-v8i8-uadd-sat-vecreduce-loop/NOTES.md) | ✅ | ❌ | ❌ | `-O2` miscomputes a loop using `llvm.uadd.sat.v8i8` followed by byte extraction and a two-lane vector-reduce xor/and idiom, changing the low bits by two. |
| [m049-vector-fshl-zero](known-miscompiles/m049-vector-fshl-zero/NOTES.md) | ✅ | ❌ | ❌ | `-O0` lowers vector `llvm.fshl.v4i32(x, 0, 0)` through a 64-bit shift-by-`-1` sequence that returns zero instead of the selected vector lane. |
| [m050-bitcount-and-sub-zero](known-miscompiles/m050-bitcount-and-sub-zero/NOTES.md) | ✅ | ✅ | ✅ | `-O0` lowered `and X, (X - 0)` feeding `ctpop` through the wrong `v_bitop3_b32`; LLVM HEAD passes after llvm/llvm-project#198373. |
| [m051-vector-fshr-divergent-loop](known-miscompiles/m051-vector-fshr-divergent-loop/NOTES.md) | ✅ | ❌ | ❌ | `-O2` scalarizes a vector `llvm.fshr.v2i32` loop tail and carries one scalar inner-loop result into divergent lanes that exited earlier. |
| [m052-ternary-blend-shift](known-miscompiles/m052-ternary-blend-shift/NOTES.md) | ✅ | ❌ | ❌ | `-O0` lowers `((a ^ b) \| (b & ~(a ^ b))) & 31` as `a & 31`, dropping `b` before a funnel-shift-like expression. |
| [m053-bytedot-highbit](known-miscompiles/m053-bytedot-highbit/NOTES.md) | ✅ | ❌ | ❌ | LLVM HEAD and ROCm HEAD `-O0` lower a byte-dot/high-bit expression through a changed `v_bitop3_b32` / `v_bfi_b32` sequence that clears a high bit before a final xor. |
| [m054-i64-pair-low-add](known-miscompiles/m054-i64-pair-low-add/NOTES.md) | ❌ | ❌ | ❌ | `-O2` folds `((zext x << 32) \| 0xffff) + zext x` into a u24 multiply-add-like sequence that drops the high-half copy of `x`. |
| [m055-i64byteperm-loop-readfirstlane](known-miscompiles/m055-i64byteperm-loop-readfirstlane/NOTES.md) | ✅ | ❌ | ✅ | LLVM HEAD `-O0` miscompiles a loop-carried value depending on an i64 byte-permutation fold, returning `0xffffffff` instead of `0xff22dd00`; ROCm 7.2.3 and ROCm HEAD pass. |
| [m056-halfdot-lowbit-branch](known-miscompiles/m056-halfdot-lowbit-branch/NOTES.md) | ✅ | ❌ | ❌ | LLVM HEAD and ROCm HEAD `-O0` miscompute a low-bit branch key derived from a halfword-dot byte pack and store zero instead of `0xfffd7ffc`. |
| [m057-rotcascade-store](known-miscompiles/m057-rotcascade-store/NOTES.md) | ✅ | ❌ | ✅ | LLVM HEAD `-O0` miscomputes a repeated rotate/popcount/bitselect cascade before the final store; ROCm 7.2.3 and ROCm HEAD pass. |
| [m058-nibble-bytesel-highbit](known-miscompiles/m058-nibble-bytesel-highbit/NOTES.md) | ❌ | ❌ | ❌ | `-O0`/`-O2` disagree on the high bit of a funnel-shift-shaped final store when a byte-lane select carry is derived from a nibble-table pack; the original oracle finding has LLVM HEAD `-O0` wrong. |
| [m059-srem-loop-branch](known-miscompiles/m059-srem-loop-branch/NOTES.md) | ✅ | ✅ | ✅ | A stale LLVM HEAD build missing llvm/llvm-project#198373 skipped a live lane when a multi-exit loop branch key came from `srem`; the current patched toolchains pass. |
| [m060-packunpack-bytedot-dot4](known-miscompiles/m060-packunpack-bytedot-dot4/NOTES.md) | ❌ | ❌ | ❌ | `-O2` folds a generated `packunpack` three-term byte-dot sum into `v_dot4_u32_u8` with the wrong packed byte or accumulator, returning `0x1e35` instead of `0x1f98`. |
| [m061-ovmaskpack-o0-overflow-lowering](known-miscompiles/m061-ovmaskpack-o0-overflow-lowering/NOTES.md) | ✅ | ❌ | ❌ | LLVM HEAD and ROCm HEAD `-O0` mislower an unoptimized overflow-mask-pack chain and store `0xa1df8800` instead of the oracle/`-O2` result `0xa0df8400`; ROCm 7.2.3 passes. |
| [m062-bytehist-bitmux-lowbyte](known-miscompiles/m062-bytehist-bitmux-lowbyte/NOTES.md) | ✅ | ❌ | ❌ | LLVM HEAD and ROCm HEAD `-O0` lower a bytehist/bitmux low-byte expression through `v_bitop3_b32` and store `0xb81c0001` instead of the oracle/`-O2` result `0xb81c0002`; ROCm 7.2.3 passes. |
| [m063-overflow-carry-bitop3](known-miscompiles/m063-overflow-carry-bitop3/NOTES.md) | ✅ | ❌ | ❌ | LLVM HEAD and ROCm HEAD `-O0` lower an overflow-derived duplicated carry expression through `v_bitop3_b32` and store `0x6` instead of the oracle/`-O2` result `0x2`; ROCm 7.2.3 passes. |
| [m064-nibblecarry-loop-readfirstlane](known-miscompiles/m064-nibblecarry-loop-readfirstlane/NOTES.md) | ✅ | ❌ | ❌ | LLVM HEAD and ROCm HEAD `-O0` scalarize a divergent nibble-carry-derived loop value through `v_readfirstlane_b32` and store `0x1805d9` instead of the oracle/`-O2` result `0xc1b09`; ROCm 7.2.3 passes. |
| [m065-usub-overflow-xor-fold](known-miscompiles/m065-usub-overflow-xor-fold/NOTES.md) | ✅ | ❌ | ❌ | LLVM HEAD and ROCm HEAD `-O0` fold `(lane ^ fold) & 1` after `usub.with.overflow` into a single `v_bitop3_b32` with the wrong truth table, storing `0x0` instead of the oracle/`-O2` result `0x1`; ROCm 7.2.3 passes. |
| [m066-veci16zextmul-bitop3-loop](known-miscompiles/m066-veci16zextmul-bitop3-loop/NOTES.md) | ❌ | ❌ | ❌ | `-O2` miscompiles a 12-iteration loop whose body builds `<4 x i16>` from the accumulator halves, zext-multiplies against constants, xor-reduces, smaxes two lanes, and xors the result back; exit value goes through a bitop3 cascade and stores `0x8BD601F1` instead of the oracle/`-O0` result `0x2BE83DE2`. |
| [m067-bytecondsel-and-i1-self](known-miscompiles/m067-bytecondsel-and-i1-self/NOTES.md) | ✅ | ❌ | ❌ | LLVM HEAD and ROCm HEAD `-O0` mis-lower `select i1 (and i1 X, X) c, 0` (where `X = icmp ult i32 a, 0`, always false) by evaluating the select as if the condition were true, storing `0xCE` instead of the oracle/`-O2` result `0x59`; ROCm 7.2.3 passes. |
| [m068-loop-vop3fused-umaxbitop3](known-miscompiles/m068-loop-vop3fused-umaxbitop3/NOTES.md) | ❌ | ❌ | ❌ | `-O2` miscompiles a nested loop whose accumulator is seeded from `vop3fused` + `umaxbitop3cascade` shapes, storing `0x937E` instead of the oracle/`-O0` `0x8210A05D`. |
| [m069-umaxbitop3cascade-store](known-miscompiles/m069-umaxbitop3cascade-store/NOTES.md) | ❌? | ❌ | ❌? | `-O2` miscompiles a final store whose value is `fuzz.umaxbitop3cascade.idiom.a.add`, storing `0x5C83AF47` instead of the oracle/`-O0` `0x814EF57`.  Sibling bug to m068; ROCm 7.2.3 / ROCm HEAD not yet verified. |
| [m070-scalar-fshl-shift8](known-miscompiles/m070-scalar-fshl-shift8/NOTES.md) | ✅ | ❌ | ❌ | `-O0` lowers scalar `fshl.i32(x, 0, 8)` to a 64-bit shift by `-8`, returning `x >> 24` instead of `x << 8`; same lowering family as m015/m016 but shows the bug applies to every non-zero constant shift, not just `c=1`. |
| [m071-bxorand-or-and-not-bitop3](known-miscompiles/m071-bxorand-or-and-not-bitop3/NOTES.md) | ❌ | ❌ | ❌ | `-O0` lowers `((b ^ (c & a)) | (c & a)) & ~(c & a)` to `v_bitop3_b32` with truth table `0x72` instead of `0x70`; sibling shape to m020/m023/m027 but a distinct expression that PR 198556 does not catch. |
| [m072-bitop3-tand-or-and-not-zero](known-miscompiles/m072-bitop3-tand-or-and-not-zero/NOTES.md) | ✅ | ❌ | ❌ | `-O0` lowers `((b & (a & c)) | (a & c)) & ~(a & c)` to `v_bitop3_b32` truth table `0x22` (= `c & ~a`) instead of `0x00`; the expression is a trivial zero. HEAD-only regression -- one of 54 failing shapes in the same `((X op1 T) op2 T) op3 ~T` structural family as m071. |
| [m073-bitop3-t1t2-and-or-xor](known-miscompiles/m073-bitop3-t1t2-and-or-xor/NOTES.md) | ❌ | ❌ | ❌ | `-O0` lowers `((a&b) & (a|c)) ^ ((a&b) | (a|c))` to `v_bitop3_b32` truth table `0x5e` instead of `0x1e`; an extra minterm fires at `(a=1, b=1, c=0)`. Structurally distinct from m071/m072 (5 ops, two intermediate values reused with AND/OR/XOR -- no `~T` term). |
| [m074-fmed3-nan-ieee-off-maxmin](known-miscompiles/m074-fmed3-nan-ieee-off-maxmin/NOTES.md) | ❌ | ❌ | ❌ | `-O2` InstCombine fold of `amdgcn.fmed3(x, y, NaN)` in IEEE-off mode produces `maximumnum(x, y)` instead of `minimumnum(x, y)`; the polarity check in `AMDGPUInstCombineIntrinsic.cpp` only treats `-inf` as "Min" and defaults NaN to "Max", inconsistent with both the documented behaviour table and the parallel arms for `Src0`/`Src1`. |
| [m075-rcp-constant-denormal-flush](known-miscompiles/m075-rcp-constant-denormal-flush/NOTES.md) | ❌ | ❌ | ❌ | `-O2` InstCombine fold of `amdgcn.rcp.f32(C)` returns the exact `1/C` even when the kernel's f32 denormal mode is `PreserveSign` (the default) and the hardware would have flushed the denormal result to `±0`. For `C = 2^127` the fold returns `0x00400000` while `v_rcp_f32` returns `0`. A `TODO` next to the fold already calls out this issue. |
| [c001-sudot-isel-ice](known-miscompiles/c001-sudot-isel-ice/NOTES.md) | ❌ | ❌ | ❌ | `llvm.amdgcn.sudot4` / `llvm.amdgcn.sudot8` abort in AMDGPU instruction selection with `Cannot select`. |
| [c002-fma-legacy-isel-ice](known-miscompiles/c002-fma-legacy-isel-ice/NOTES.md) | ❌ | ❌ | ❌ | `-O0` leaves `llvm.amdgcn.fma.legacy` for AMDGPU instruction selection, which aborts with `Cannot select`; `-O2` compiles the reduced case. |
| [c003-permlane16-isel-ice](known-miscompiles/c003-permlane16-isel-ice/NOTES.md) | ❌ | ❌ | ❌ | `llvm.amdgcn.permlane16` ICEs with `Cannot select` on every CDNA target (gfx9xx); the instruction is GFX10+/RDNA only but the intrinsic is declared target-unconditional. |
| [c004-mov-dpp8-isel-ice](known-miscompiles/c004-mov-dpp8-isel-ice/NOTES.md) | ❌ | ❌ | ❌ | `llvm.amdgcn.mov.dpp8` ICEs with `Cannot select` on every CDNA target; same root cause as c003 -- DPP8 is GFX10+/RDNA only. |
| [c005-global-load-lds-isel-ice](known-miscompiles/c005-global-load-lds-isel-ice/NOTES.md) | ❌ | ❌ | ❌ | `llvm.amdgcn.global.load.lds` ICEs with `Cannot select` on gfx950; same family as c003/c004. `llvm.amdgcn.ds.ordered.add` ICEs the same way (mentioned in the c005 notes rather than getting its own entry). |
| [c006-tanh-isel-ice](known-miscompiles/c006-tanh-isel-ice/NOTES.md) | ❌ | ❌ | ❌ | `llvm.amdgcn.tanh` (both `.f32` and `.f16`) ICEs with `Cannot select` on gfx950; `v_tanh_*` is a GFX12 instruction not available on CDNA. Same fix shape as c003. |
| [c007-fcmp-i32-wave64-fold-ice](known-miscompiles/c007-fcmp-i32-wave64-fold-ice/NOTES.md) | ❌ | ❌ | ❌ | `llvm.amdgcn.fcmp.i32` with two equal constant FP operands ICEs at `-O2` on any wave64 target with `invalid type for register "exec"`; the constant folder doesn't validate that the requested return width matches the wave size. Distinct shape from c003--c006 -- a constant-folder bug rather than a missing subtarget predicate. |

*Human-written note:* Up through bug m016 I was testing against upstream LLVM.
But then it became clear that the ROCm 7.2.3 release didn't have many of these
bugs, so I switched to testing the release.  After m038, AMD asked me to switch
fuzzing back to upstream.

## LLVM Source Builds

The fuzzer can use an installed ROCm LLVM today.  For coverage-guided compiler
fuzzing, initialize the LLVM submodule and build an instrumented LLVM.  To use a
different LLVM checkout or fork, set `LLVM_PROJECT_DIR=/path/to/llvm-project`.

Typical directed-fuzzing setup:

```bash
git submodule update --init --depth 1 third_party/llvm-project
scripts/build_instrumented_llvm.sh
scripts/build_directed_fuzzer.sh
scripts/run_directed_fuzzer.sh
```

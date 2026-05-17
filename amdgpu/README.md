# FuzzX AMDGPU

This directory contains the AMDGPU fuzzer work area.  It is intentionally
separate from the PTX / `ptxas` fuzzer in [`../ptx/`](../ptx/).

The AMDGPU fuzzer is the directed C++ libFuzzer target in `fuzzer/`. Its only
input format is an LLVM bitcode module containing an AMDGPU kernel named
`fuzz_kernel`. For each input module, the fuzzer compiles the kernel through
`-O0` and `-O2` LLVM pipelines, links both code objects into one HSACO, runs
both kernels through HIP, and compares device output.

The custom mutator and crossover operate on LLVM IR rather than on raw bytes.
They currently build a conservative, defined subset of integer IR: no `undef`,
no explicit poison values, no `nuw` / `nsw` / `exact`, no `inbounds`, no
integer division except nonzero-denominator `udiv` / `urem`, only masked or
constant shift amounts, and only the fixed skeleton input load/output store.
Coverage includes scalar integer arithmetic, bitwise ops, compares/selects,
`i64` subexpressions truncated to `i32`, `<2 x i32>` / `<4 x i32>` vector
subexpressions reduced back to `i32`, explicit `i1` boolean subexpressions
reduced back to `i32`, and LLVM bit, min/max, saturation, absolute-value, and
funnel-shift intrinsics. The mutator can also wrap the current result in
structured two-way branches, wider multi-way switches, and deeper bounded CFG
subgraphs with `i32` phi joins. Those subgraphs can nest more diamonds,
switches, and small leaf counted loops with optional guarded early exits. The
mutator also generates top-level counted loops with small bounded constant or
dynamically masked trip counts whose bodies can contain nested diamonds,
switches, and inner loops. Some generated loops carry two independent `i32`
accumulator phis, combine them after the loop, or take a guarded early exit from
the loop body through an exit phi, so corpus entries exercise both expression
simplification and CFG and loop transforms. CFG arms include the same scalar
integer, bit, boolean, narrowing, saturating, funnel-shift, and vector expression
families as the linear mutator.
Corpus files can be inspected directly with `opt -S corpus-entry -o -`.

## Requirements

| Component | Notes |
| --- | --- |
| ROCm LLVM | Defaults to `/opt/rocm-7.1.1/lib/llvm/bin/clang-20`, `lld`, and `llvm-objdump`; override with `CLANG`, `LLD`, and `LLVM_OBJDUMP`. |
| HIP | `hipcc` is used to build the module runner. |
| AMDGPU | Defaults to `gfx950`; override with `--mcpu`. |

## Run

Build and run the directed C++ GPU differential fuzzer:

```bash
scripts/build_directed_fuzzer.sh
HIP_DEVICE=0 scripts/run_directed_fuzzer.sh -runs=100 -max_len=65536
```

Run one directed fuzzer process per GPU:

```bash
scripts/run_directed_multigpu_fuzzer.sh -runs=1000 -max_len=65536
```

Run multiple directed fuzzer workers on each selected GPU:

```bash
WORKERS_PER_GPU=2 scripts/run_directed_multigpu_fuzzer.sh -runs=1000 -max_len=65536
```

Multi-GPU runs share one live libFuzzer corpus by default, so workers can
reload inputs discovered by other workers while keeping per-worker logs and
artifact directories. Set `FUZZX_CORPUS_MODE=isolated` to return to one
independent corpus directory per worker.
Fresh corpus directories are seeded with a valid LLVM bitcode module before
libFuzzer starts.

With an optimized ROCm 7.2.3 LLVM build using sanitizer coverage and no ASan,
the directed fuzzer currently reaches about 500 exec/s aggregate across 8 GPUs.
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

For ROCm 7.2.3 release fuzzing, use the release wrapper:

```bash
scripts/run_rocm_7_2_3_release_fuzzer.sh -max_total_time=900 -max_len=65536 -rss_limit_mb=8192 -use_value_profile=1
```

That wrapper selects the ROCm 7.2.3 fuzzer build.

Candidate compiler crashes, runner failures, or output mismatches are saved
under `$FUZZX_RUNTIME_ROOT/findings` by default. Generated corpora and findings
are local artifacts and are ignored by git; set `FUZZX_RUNTIME_ROOT`,
`CORPUS_ROOT`, `LOG_DIR`, `ARTIFACT_ROOT`, or `FUZZX_FINDINGS_DIR` to override
the default local runtime paths.

### Known-Bug Suppression

Known bug patterns are suppressed by default so continued fuzzing does not keep
rediscovering the same issue.

| Flag | Default | Meaning |
| --- | --- | --- |
| `FUZZX_ALLOW_M019_HIGHBIT_OR_XOR=1` | unset | Re-enable the outer high-bit `(x | C) ^ x` shape for [m019](known-miscompiles/m019-highbit-or-xor/NOTES.md). |
| `FUZZX_ALLOW_M020_OR_XOR_AND=1` | unset | Re-enable the `((a | b) ^ b) & (a | b)` shape for [m020](known-miscompiles/m020-or-xor-and/NOTES.md). |
| `FUZZX_ALLOW_M021_OR_XOR=1` | unset | Re-enable the generalized dynamic `(a | b) ^ a` shape for [m021](known-miscompiles/m021-fshl-or-xor/NOTES.md). |
| `FUZZX_ALLOW_M022_AND_XOR_CONSTANT=1` | unset | Re-enable the `((x ^ C) & x)` shape for [m022](known-miscompiles/m022-and-xor-constant/NOTES.md). |
| `FUZZX_ALLOW_M023_AND_XOR_IDENTITY=1` | unset | Re-enable the `(x & y) ^ x` shape for [m023](known-miscompiles/m023-and-xor-identity/NOTES.md). |
| `FUZZX_ALLOW_M024_UDIV_SEXT_OR=1` | unset | Re-enable unsigned division by odd `or` denominators for [m024](known-miscompiles/m024-udiv-or-one/NOTES.md). |
| `FUZZX_ALLOW_M025_UREM_SEXT_OR=1` | unset | Re-enable unsigned remainder by odd `or` denominators for [m025](known-miscompiles/m025-urem-or-one/NOTES.md). |
| `FUZZX_ALLOW_M026_UMAX_XOR_AND_HIGHBIT=1` | unset | Re-enable `(umax(a, b) ^ b) & umax(a, b)` shapes for [m026](known-miscompiles/m026-shl-umax-xor-and/NOTES.md). |
| `FUZZX_ALLOW_M027_XOR_AND_OR=1` | unset | Re-enable `(((y ^ x) & x) | base)` when `x` is `(base ^ z) & base` for [m027](known-miscompiles/m027-xor-and-or/NOTES.md). |
| `FUZZX_ALLOW_M028_UMAX_AND_NOT=1` | unset | Re-enable `(umax((y & ~x), C) & x) & ~x` shapes for [m028](known-miscompiles/m028-umax-and-not/NOTES.md). |
| `FUZZX_ALLOW_M029_FSHL_SELECT_PHI=1` | unset | Re-enable signed compare/select or compare/PHI shapes over `(y & x)` where `x` is a complemented masked `fshl` for [m029](known-miscompiles/m029-fshl-select-phi/NOTES.md). |
| `FUZZX_ALLOW_M030_CTLZ_SHL_OR_BITOP3=1` | unset | Re-enable `or(add(shl(...), z), z)` and `or(smin(add(shl(...), z), z), z)` tails for [m030](known-miscompiles/m030-ctlz-shl-or-bitop3/NOTES.md). |
| `FUZZX_ALLOW_M031_VECTOR_OR_EXTRACT_SUB=1` | unset | Re-enable subtracting two scalar extracts from the same vector `or` for [m031](known-miscompiles/m031-vector-or-extract-sub/NOTES.md). |
| `FUZZX_ALLOW_M032_LOOP_VECTOR_SELECT=1` | unset | Re-enable loop-carried values whose backedge depends on a vector `select` for [m032](known-miscompiles/m032-loop-vector-select/NOTES.md). |

## Layout

| Path | Purpose |
| --- | --- |
| `third_party/llvm-project` | LLVM source checkout, pinned as a git submodule. |
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

Tested toolchains as of 2026-05-17:

| Column | Toolchain |
| --- | --- |
| ROCm release | [ROCm 7.2.3 source tag](https://github.com/ROCm/llvm-project/releases/tag/rocm-7.2.3), commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`; also checked against the matching [ROCm 7.2.3 `rocm-llvm` package](https://repo.radeon.com/rocm/apt/7.2.3/pool/main/r/rocm-llvm/rocm-llvm_22.0.0.26084.70203-90~22.04_amd64.deb), package SHA256 `4c406e184f88949cea60869949454e5392e1cbd9480c4c87274f7b59e9f810e5`. |
| LLVM HEAD | https://github.com/llvm/llvm-project/commit/10756d32f96154f0889eda159ea9a26bc4188bda (2026-05-16), built with assertions, ASan, and sanitizer coverage. |
| ROCm HEAD | https://github.com/ROCm/llvm-project/commit/9115c466b3577830455f70c4f492429bf6c64b25 (2026-05-16), built with assertions, ASan, and sanitizer coverage. |

| Bug | ROCm 7.2.3 | LLVM HEAD | ROCm HEAD | Description |
| --- | --- | --- | --- | --- |
| [m001-ashr-i16-zext](known-miscompiles/m001-ashr-i16-zext/NOTES.md) | ❌ | ❌ | ❌ | `ashr i16` feeding `zext i16 to i32` is folded to a sign-extending SDWA byte select. |
| [m002-i8-clear-xor](known-miscompiles/m002-i8-clear-xor/NOTES.md) | ✅ | ❌ | ❌ | `-O0` lowers a byte-clear xor through `v_bitop3_b32` with the wrong result. |
| [m003-shl3-add-chain](known-miscompiles/m003-shl3-add-chain/NOTES.md) | ✅ | ❌ | ❌ | `-O0` scalarizes a divergent `shl3/add` chain through `v_readfirstlane_b32`. |
| [m004-vector-identity-xor](known-miscompiles/m004-vector-identity-xor/NOTES.md) | ✅ | ❌ | ❌ | `-O0` loses a lane-0 vector identity before `xor`. |
| [m005-shl1-add-chain](known-miscompiles/m005-shl1-add-chain/NOTES.md) | ✅ | ❌ | ❌ | `-O0` scalarizes a divergent `shl1/add` chain through the same class of bug as m003. |
| [m006-i8-xor-clear](known-miscompiles/m006-i8-xor-clear/NOTES.md) | ✅ | ❌ | ❌ | `-O0` lowers another adjacent `i8` narrow byte-clear xor through the wrong `v_bitop3_b32` result. |
| [m007-vector-shl-identity-xor](known-miscompiles/m007-vector-shl-identity-xor/NOTES.md) | ✅ | ❌ | ❌ | `-O0` loses a vector shift-by-zero lane-0 identity before `xor`. |
| [m008-i8-separated-clear](known-miscompiles/m008-i8-separated-clear/NOTES.md) | ✅ | ❌ | ❌ | `-O0` miscompiles an `i8` identity byte-clear xor when prior narrow ops are separated by no-op adds. |
| [m009-i16-clear-xor](known-miscompiles/m009-i16-clear-xor/NOTES.md) | ✅ | ❌ | ❌ | `-O0` miscompiles an `i16` identity low-16 clear xor through the wrong `v_bitop3_b32` result. |
| [m010-i16-sext-clear-xor](known-miscompiles/m010-i16-sext-clear-xor/NOTES.md) | ✅ | ❌ | ❌ | `-O0` miscompiles an `i16` sign-extended identity clear xor through the wrong `v_bitop3_b32` result. |
| [m011-i8-sext-clear-xor](known-miscompiles/m011-i8-sext-clear-xor/NOTES.md) | ✅ | ❌ | ❌ | `-O0` miscompiles an `i8` sign-extended masked clear xor through the wrong `v_bitop3_b32` result. |
| [m012-add-shl-ladder](known-miscompiles/m012-add-shl-ladder/NOTES.md) | ✅ | ❌ | ❌ | `-O0` scalarizes a divergent `add/shl` ladder through `v_readfirstlane_b32`. |
| [m013-private-memory-fshl](known-miscompiles/m013-private-memory-fshl/NOTES.md) | ❌ | ❌ | ❌ | `-O0` lowers fixed private-memory allocas through a dynamic scratch stack sequence that can return intermittent wrong values. |
| [m014-shl-add-ctpop](known-miscompiles/m014-shl-add-ctpop/NOTES.md) | ✅ | ❌ | ❌ | `-O0` scalarizes a four-step `shl/add` chain feeding `ctpop` through lane 0. |
| [m015-scalar-fshl-zero](known-miscompiles/m015-scalar-fshl-zero/NOTES.md) | ✅ | ❌ | ❌ | `-O0` lowers scalar `fshl.i32(x, y, 0)` through a 64-bit shift-by-`-1` sequence that returns zero. |
| [m016-scalar-fshl-one](known-miscompiles/m016-scalar-fshl-one/NOTES.md) | ✅ | ❌ | ❌ | `-O0` lowers scalar `fshl.i32(x, y, 1)` through a 64-bit shift-by-`-1` sequence that returns only bit 31. |
| [m017-vector-and-lane0-clear-xor](known-miscompiles/m017-vector-and-lane0-clear-xor/NOTES.md) | ❌ | ✅ | ✅ | ROCm 7.2.3 `-O0` drops a vector lane-0 `and`/`extractelement` clear before `xor`; LLVM HEAD and ROCm HEAD already pass. |
| [m018-two-private-memory-ops](known-miscompiles/m018-two-private-memory-ops/NOTES.md) | ❌ | ✅ | ✅ | ROCm 7.2.3 `-O0` intermittently reads stale scratch data across two private-memory sequences; LLVM HEAD and ROCm HEAD pass 50 repeated combined runs. |
| [m019-highbit-or-xor](known-miscompiles/m019-highbit-or-xor/NOTES.md) | ❌ | ❌ | ❌ | `-O0` combines a high-bit `(x | C) ^ x` expression into `v_bitop3_b32` with the wrong truth table or operands. |
| [m020-or-xor-and](known-miscompiles/m020-or-xor-and/NOTES.md) | ❌ | ❌ | ❌ | `-O0` combines `((a | b) ^ b) & (a | b)` into `v_bitop3_b32` with the wrong result. |
| [m021-fshl-or-xor](known-miscompiles/m021-fshl-or-xor/NOTES.md) | ❌ | ❌ | ❌ | `-O0` combines a dynamic `(a | b) ^ a` expression after `fshl` into `v_bitop3_b32` with the wrong result. |
| [m022-and-xor-constant](known-miscompiles/m022-and-xor-constant/NOTES.md) | ❌ | ❌ | ❌ | `-O0` combines `((x ^ C) & x)` after a dynamic `and` into `v_bitop3_b32` with the wrong low bit. |
| [m023-and-xor-identity](known-miscompiles/m023-and-xor-identity/NOTES.md) | ❌ | ❌ | ❌ | `-O0` combines `(x & y) ^ x` into `v_bitop3_b32` with the wrong identity result. |
| [m024-udiv-or-one](known-miscompiles/m024-udiv-or-one/NOTES.md) | ❌ | ❌ | ❌ | `-O0` lowers unsigned division of a sign-extended `i16` value by `x | 1` through an imprecise float reciprocal path. |
| [m025-urem-or-one](known-miscompiles/m025-urem-or-one/NOTES.md) | ❌ | ❌ | ❌ | `-O0` lowers unsigned remainder of a sign-extended `i16` value by `x | 1` through the same imprecise reciprocal path. |
| [m026-shl-umax-xor-and](known-miscompiles/m026-shl-umax-xor-and/NOTES.md) | ❌ | ❌ | ❌ | `-O2` combines a shifted `umax` high-bit extraction into `v_bitop3_b32` using the input and salt instead of their xor. |
| [m027-xor-and-or](known-miscompiles/m027-xor-and-or/NOTES.md) | ❌ | ❌ | ❌ | `-O0` combines `(((y ^ x) & x) | base)` into `v_bitop3_b32` with the wrong bit when `x` is `(base ^ z) & base`. |
| [m028-umax-and-not](known-miscompiles/m028-umax-and-not/NOTES.md) | ❌ | ❌ | ❌ | `-O0` combines `(umax((y & ~x), C) & x) & ~x` into `v_bitop3_b32` using the input and salt separately. |
| [m029-fshl-select-phi](known-miscompiles/m029-fshl-select-phi/NOTES.md) | ❌ | ❌ | ❌ | `-O2` lowers a signed compare/select over `y & x`, where `x` is a complemented masked `fshl`, so the true zero arm is chosen when the signed compare is false. |
| [m030-ctlz-shl-or-bitop3](known-miscompiles/m030-ctlz-shl-or-bitop3/NOTES.md) | ❌ | ❌ | ❌ | `-O2` lowers a low-bit `or` through `v_bitop3_b32` using the unmasked `%n` value instead of `%n & 1`. |
| [m031-vector-or-extract-sub](known-miscompiles/m031-vector-or-extract-sub/NOTES.md) | ❌ | ✅ | ✅ | ROCm 7.2.3 `-O2` scalarizes a vector `or` extract/sub as `(x | 255) - x` instead of `(x | 255) - -1`. |
| [m032-loop-vector-select](known-miscompiles/m032-loop-vector-select/NOTES.md) | ❌ | ✅ | ✅ | ROCm 7.2.3 `-O2` kills the loop EXEC mask before storing a loop-carried value derived from a vector `select`. |

*Human-written note:* Up through bug m016 I was testing against upstream LLVM.  But then it became clear that the ROCm 7.2.3 release doesn't have most of the bugs that are appearing in upstream.  I'm more interested in bugs that appear in the release, so after this, I started testing against 7.2.3 (built from source).

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

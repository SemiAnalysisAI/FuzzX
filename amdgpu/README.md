# FuzzX AMDGPU

This directory contains the AMDGPU fuzzer work area.  It is intentionally
separate from the PTX / `ptxas` fuzzer in [`../ptx/`](../ptx/).

The current fuzzer is the directed C++ libFuzzer target in
`fuzzer/`. It builds restricted LLVM IR through LLVM's C++ API,
compiles the IR to AMDGPU code objects through `-O0` and `-O2` LLVM pipelines,
runs both through HIP, and compares device output.  The generator emits only
operations with defined LLVM semantics: no `undef`, no `poison`, no `nuw` /
`nsw` / `exact`, no `inbounds`, no division, and all shift amounts are constants
below the shifted value's bit width.  Coverage includes scalar integer ops,
small-width integer ops, packed `i8` / `i16` vectors, selects, structured CFG,
private-memory, LDS/local-memory, and global scratch load/store sequences, and
deterministic per-workitem LDS/global `atomicrmw` sequences, and LLVM overflow,
saturation, bit, and
funnel-shift intrinsics across scalar, small-width, and widened integer types.
The generated IR also covers narrow min/max intrinsics, widened compare /
select and `fshr` paths, and masked dynamic shifts across scalar and narrow
integer widths. Packed-vector coverage includes saturating arithmetic, bit
intrinsics, and masked dynamic shifts. Wider `i32` vectors cover min/max, bit
intrinsics, masked dynamic shifts, vector `fshr`, and vectors whose lanes are
all derived from the live input value. Floating-point coverage includes finite
`half`, `float`, and `double` arithmetic, comparisons, `fabs`, `sqrt`, `fma`,
`fdiv`, and `double`-to-`float` rounding; the generator keeps the values
bounded and mixes them back through bitcasts to avoid FP-to-integer poison.
Set `FUZZX_ENABLE_ORACLE=1` to also compare GPU output against the fuzzer's
integer/bit/vector/private-memory/LDS/global-memory/atomic semantic oracle.
FP operations currently remain differential-only in oracle mode.

The corpus format is a compact binary encoding of this restricted `Program`
model, not textual LLVM IR. The fuzzer provides `LLVMFuzzerCustomMutator` and
`LLVMFuzzerCustomCrossOver` so libFuzzer mutates whole operations, operation
constants, CFG controls, and operation ranges instead of mostly corrupting byte
boundaries.

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
HIP_DEVICE=0 scripts/run_directed_fuzzer.sh -runs=100 -max_len=512
```

Run one directed fuzzer process per GPU:

```bash
scripts/run_directed_multigpu_fuzzer.sh -runs=1000 -max_len=512
```

Run multiple directed fuzzer workers on each selected GPU:

```bash
WORKERS_PER_GPU=2 scripts/run_directed_multigpu_fuzzer.sh -runs=1000 -max_len=512
```

Multi-GPU runs share one live libFuzzer corpus by default, so workers can
reload inputs discovered by other workers while keeping per-worker logs and
artifact directories. Set `FUZZX_CORPUS_MODE=isolated` to return to one
independent corpus directory per worker.

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
scripts/run_rocm_7_2_3_release_fuzzer.sh -max_total_time=900 -max_len=1024 -rss_limit_mb=8192 -use_value_profile=1
```

To look for lowering bugs shared by both optimization levels, enable the
expected-output oracle:

```bash
FUZZX_ENABLE_ORACLE=1 scripts/run_rocm_7_2_3_release_fuzzer.sh -max_total_time=900 -max_len=1024 -rss_limit_mb=8192 -use_value_profile=1
```

That wrapper keeps the release-reproducing bugs suppressed (`m001`, `m013`,
`m017`, and `m018`), but explicitly re-enables the idioms that only failed on
LLVM HEAD / ROCm HEAD in the checked matrix (`m002` through `m012`, plus `m014`
through `m016`).

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
| `FUZZX_ALLOW_M001_ASHR_I16_ZEXT=1` | unset | Re-enable the directed C++ fuzzer shape for [m001](known-miscompiles/m001-ashr-i16-zext/NOTES.md). |
| `FUZZX_ALLOW_M002_I8_CLEAR_XOR=1` | unset | Re-enable the adjacent `i8` narrow/xor shape for [m002](known-miscompiles/m002-i8-clear-xor/NOTES.md). |
| `FUZZX_ALLOW_M003_SHL3_ADD_CHAIN=1` | unset | Re-enable the five-step `shl/add` chain shape found by [m003](known-miscompiles/m003-shl3-add-chain/NOTES.md). |
| `FUZZX_ALLOW_M004_VECTOR_IDENTITY_XOR=1` | unset | Re-enable vector lane-0 identity xor shapes including [m004](known-miscompiles/m004-vector-identity-xor/NOTES.md) and [m017](known-miscompiles/m017-vector-and-lane0-clear-xor/NOTES.md). |
| `FUZZX_ALLOW_M005_SHL_ADD_CHAIN=1` | unset | Alias to re-enable the broader five-step `shl/add` chain shape for [m005](known-miscompiles/m005-shl1-add-chain/NOTES.md). |
| `FUZZX_ALLOW_M006_I8_CLEAR_XOR=1` | unset | Alias to re-enable the broader adjacent `i8` narrow/xor shape for [m006](known-miscompiles/m006-i8-xor-clear/NOTES.md). |
| `FUZZX_ALLOW_M007_VECTOR_IDENTITY_XOR=1` | unset | Alias to re-enable the broader vector lane-0 identity xor shape for [m007](known-miscompiles/m007-vector-shl-identity-xor/NOTES.md). |
| `FUZZX_ALLOW_M008_I8_CLEAR_XOR=1` | unset | Alias to re-enable the broader `i8` identity byte-clear xor shape for [m008](known-miscompiles/m008-i8-separated-clear/NOTES.md). |
| `FUZZX_ALLOW_M009_I16_CLEAR_XOR=1` | unset | Re-enable the `i16` identity low-16 clear xor shape for [m009](known-miscompiles/m009-i16-clear-xor/NOTES.md). |
| `FUZZX_ALLOW_M010_I16_SEXT_CLEAR_XOR=1` | unset | Re-enable the `i16` sign-extended identity clear xor shape for [m010](known-miscompiles/m010-i16-sext-clear-xor/NOTES.md). |
| `FUZZX_ALLOW_M011_I8_SEXT_CLEAR_XOR=1` | unset | Re-enable the `i8` sign-extended identity clear xor shape for [m011](known-miscompiles/m011-i8-sext-clear-xor/NOTES.md). |
| `FUZZX_ALLOW_M012_ADD_SHL_LADDER=1` | unset | Alias to re-enable the broader `add/shl` ladder shape for [m012](known-miscompiles/m012-add-shl-ladder/NOTES.md). |
| `FUZZX_ALLOW_M013_PRIVATE_MEMORY_FSHL=1` | unset | Re-enable three-or-more private-memory/funnel-shift ops for [m013](known-miscompiles/m013-private-memory-fshl/NOTES.md). |
| `FUZZX_ALLOW_M014_SHL_ADD_CTPOP=1` | unset | Re-enable four-step `shl/add` chains feeding `ctpop` for [m014](known-miscompiles/m014-shl-add-ctpop/NOTES.md). |
| `FUZZX_ALLOW_M015_SCALAR_FSHL_ZERO=1` | unset | Re-enable zero-count `fshl` generation for [m015](known-miscompiles/m015-scalar-fshl-zero/NOTES.md); this also permits generated `fshl` after m016. |
| `FUZZX_ALLOW_M016_SCALAR_FSHL=1` | unset | Re-enable nonzero scalar `fshl` generation for [m016](known-miscompiles/m016-scalar-fshl-one/NOTES.md). |
| `FUZZX_ALLOW_M017_VECTOR_AND_LANE0_CLEAR_XOR=1` | unset | Re-enable vector lane-0 `and`/`extractelement` clear-xor shapes for [m017](known-miscompiles/m017-vector-and-lane0-clear-xor/NOTES.md). |
| `FUZZX_ALLOW_M018_TWO_PRIVATE_MEMORY_OPS=1` | unset | Re-enable programs with two private-memory operations for [m018](known-miscompiles/m018-two-private-memory-ops/NOTES.md). |

## Layout

| Path | Purpose |
| --- | --- |
| `third_party/llvm-project` | LLVM source checkout, pinned as a git submodule. |
| `scripts/build_instrumented_llvm.sh` | Helper for configuring a sanitizer-coverage LLVM source build. |
| `scripts/build_directed_fuzzer.sh` | Builds the C++ GPU differential libFuzzer target. |
| `scripts/run_directed_fuzzer.sh` | Runs the C++ directed fuzzer on one GPU. |
| `scripts/run_directed_multigpu_fuzzer.sh` | Runs one or more C++ directed fuzzer processes per selected GPU. |
| `scripts/run_rocm_7_2_3_release_fuzzer.sh` | Runs the C++ directed fuzzer with the ROCm 7.2.3 release suppression policy. |
| `fuzzer/` | LLVM API plus HIP differential libFuzzer target. |
| `runner/hip_module_runner.cpp` | HIP module loader used to execute generated HSACO files. |
| `known-miscompiles/` | Reduced or standalone reproducers for confirmed findings. |

## AMDGPU Bugs Found

Except where otherwise noted, these have been tested on `gfx950`.  The result
columns report the generic `known-miscompiles/run_ll_reproducer.sh`
differential test: ✅ means no mismatch was observed for that reproducer, and
❌ means the toolchain reproduces the `-O0` / `-O2` mismatch.

Tested toolchains as of 2026-05-16:

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

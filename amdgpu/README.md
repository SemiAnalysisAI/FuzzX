# FuzzX AMDGPU

This directory contains the AMDGPU fuzzer work area.  It is intentionally
separate from the PTX / `ptxas` fuzzer in [`../ptx/`](../ptx/).

The current fuzzer is the directed C++ libFuzzer target in
`fuzzers/llvm-amdgpu-diff`. It builds restricted LLVM IR through LLVM's C++ API,
compiles the IR to AMDGPU code objects through `-O0` and `-O2` LLVM pipelines,
runs both through HIP, and compares device output.  The generator emits only
operations with defined LLVM semantics: no `undef`, no `poison`, no `nuw` /
`nsw` / `exact`, no `inbounds`, no division, and all shift amounts are constants
below the shifted value's bit width.

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

In the current setup the directed fuzzer ran 16,000 executions in 106 seconds
across 8 GPUs, about 151 exec/s aggregate, while libFuzzer receives coverage
from the instrumented LLVM codegen path for each input.

Candidate compiler crashes, runner failures, or output mismatches are saved
under `findings/`.

### Known-Bug Suppression

Known bug patterns are suppressed by default so continued fuzzing does not keep
rediscovering the same issue.

| Flag | Default | Meaning |
| --- | --- | --- |
| `FUZZX_ALLOW_M001_ASHR_I16_ZEXT=1` | unset | Re-enable the directed C++ fuzzer shape for [m001](known-miscompiles/m001-ashr-i16-zext/NOTES.md). |
| `FUZZX_ALLOW_M002_I8_CLEAR_XOR=1` | unset | Re-enable the adjacent `i8` narrow/xor shape for [m002](known-miscompiles/m002-i8-clear-xor/NOTES.md). |
| `FUZZX_ALLOW_M003_SHL3_ADD_CHAIN=1` | unset | Re-enable the five-step `shl/add` chain shape found by [m003](known-miscompiles/m003-shl3-add-chain/NOTES.md). |
| `FUZZX_ALLOW_M004_VECTOR_IDENTITY_XOR=1` | unset | Re-enable the vector lane-0 identity xor shape for [m004](known-miscompiles/m004-vector-identity-xor/NOTES.md). |
| `FUZZX_ALLOW_M005_SHL_ADD_CHAIN=1` | unset | Alias to re-enable the broader five-step `shl/add` chain shape for [m005](known-miscompiles/m005-shl1-add-chain/NOTES.md). |
| `FUZZX_ALLOW_M006_I8_CLEAR_XOR=1` | unset | Alias to re-enable the broader adjacent `i8` narrow/xor shape for [m006](known-miscompiles/m006-i8-xor-clear/NOTES.md). |

## Layout

| Path | Purpose |
| --- | --- |
| `scripts/fetch_llvm.sh` | Shallow LLVM checkout helper. |
| `scripts/build_instrumented_llvm.sh` | Helper for configuring a sanitizer-coverage LLVM source build. |
| `scripts/build_directed_fuzzer.sh` | Builds the C++ GPU differential libFuzzer target. |
| `scripts/run_directed_fuzzer.sh` | Runs the C++ directed fuzzer on one GPU. |
| `scripts/run_directed_multigpu_fuzzer.sh` | Runs one C++ directed fuzzer process per selected GPU. |
| `fuzzers/llvm-amdgpu-diff/` | LLVM API plus HIP differential libFuzzer target. |
| `runner/hip_module_runner.cpp` | HIP module loader used to execute generated HSACO files. |
| `known-miscompiles/` | Reduced or standalone reproducers for confirmed findings. |
| `findings/` | Saved candidate bugs. |
| `corpus/` | Reserved for future coverage-guided corpus inputs. |

## AMDGPU Bugs Found

Except where otherwise noted, these have been tested on `gfx950`.

Version | Description |
| --- | --- |
| ROCm 7.1.1 / LLVM 23.0.0git | [m001-ashr-i16-zext](known-miscompiles/m001-ashr-i16-zext/NOTES.md): `ashr i16` feeding `zext i16 to i32` is folded to a sign-extending SDWA byte select. |
| LLVM 23.0.0git | [m002-i8-clear-xor](known-miscompiles/m002-i8-clear-xor/NOTES.md): `-O0` lowers a byte-clear xor through `v_bitop3_b32` with the wrong result. |
| LLVM 23.0.0git | [m003-shl3-add-chain](known-miscompiles/m003-shl3-add-chain/NOTES.md): `-O0` scalarizes a divergent `shl3/add` chain through `v_readfirstlane_b32`. |
| LLVM 23.0.0git | [m004-vector-identity-xor](known-miscompiles/m004-vector-identity-xor/NOTES.md): `-O0` loses a lane-0 vector identity before `xor`. |
| LLVM 23.0.0git | [m005-shl1-add-chain](known-miscompiles/m005-shl1-add-chain/NOTES.md): `-O0` scalarizes a divergent `shl1/add` chain through the same class of bug as m003. |
| LLVM 23.0.0git | [m006-i8-xor-clear](known-miscompiles/m006-i8-xor-clear/NOTES.md): `-O0` lowers another adjacent `i8` narrow byte-clear xor through the wrong `v_bitop3_b32` result. |

## LLVM Source Builds

The fuzzer can use an installed ROCm LLVM today.  For coverage-guided compiler
fuzzing, point `scripts/build_instrumented_llvm.sh` at an LLVM source checkout
or fork with `LLVM_PROJECT_DIR=/path/to/llvm-project`; this repo does not
currently vendor LLVM as a submodule.

Typical directed-fuzzing setup:

```bash
scripts/fetch_llvm.sh
LLVM_PROJECT_DIR=$PWD/third_party/llvm-project scripts/build_instrumented_llvm.sh
scripts/build_directed_fuzzer.sh
scripts/run_directed_fuzzer.sh
```

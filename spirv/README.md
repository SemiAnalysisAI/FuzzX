# FuzzX SPIR-V

This directory mirrors the [`amdgpu/`](../amdgpu/) layout for the LLVM
SPIR-V backend.

## How this differs from AMDGPU / PTX (read this first)

The AMDGPU and PTX fuzzers are **differential-execution** fuzzers: they
generate UB-free programs, compile them, run them on a real GPU through
HIP / the CUDA driver, and compare `-O0` vs `-O2` vs an interpreter
oracle.  That's only possible because each backend has a vendor-provided
host runtime that loads and executes its output, already installed on
the box.

SPIR-V has no equivalent.  Executing what the LLVM SPIR-V backend emits
requires either:

- a Vulkan compute pipeline (Vulkan ICD + descriptor/dispatch boilerplate),
- an OpenCL ICD that accepts LLVM's SPIR-V flavor (e.g. PoCL), or
- SPIRV-Cross translation back to GLSL/HLSL/MSL plus a downstream compiler.

None is a drop-in equivalent of `libamdhip64` / `libcuda`, and the LLVM
SPIR-V backend itself has no `-O0` vs `-O2` story to diff against (both
go through the same backend pipeline).

So this fuzzer is **crash-only**: it runs the SPIR-V codegen pipeline
in-process and reports any assertion failure, `report_fatal_error`,
`UNREACHABLE`, segfault, or `CrashRecoveryContext`-trapped abort as a
libFuzzer crash.  Everything else — libFuzzer entry, coverage-guided
input mutation, in-process `TargetMachine`, fatal-error handler routed
to `std::abort`, seed-corpus bitcode, the `build_instrumented_llvm.sh`
/ `build_directed_fuzzer.sh` / `run_directed_fuzzer.sh` script trio —
is mirrored straight from `amdgpu/`.

## Layout

| Path | Purpose |
| --- | --- |
| `fuzzer/llvm_spirv_crash_fuzzer.cpp` | libFuzzer target. Parses input as bitcode, runs the SPIR-V codegen pipeline under `CrashRecoveryContext`, aborts on any backend ICE. |
| `fuzzer/CMakeLists.txt` | Same shape as `amdgpu/fuzzer/CMakeLists.txt` minus LLD / HIP. |
| `scripts/build_instrumented_llvm.sh` | Builds LLVM with assertions, `SPIRV;X86` targets, and sancov for coverage feedback. |
| `scripts/build_directed_fuzzer.sh` | Builds the libFuzzer target against the instrumented LLVM. |
| `scripts/run_directed_fuzzer.sh` | Seeds the corpus and runs the fuzzer; identical flow to the AMDGPU script. |
| `scripts/seed_ir_corpus.sh` | Emits a single `spirv64` bitcode seed. |
| `third_party/llvm-project/` | Place an llvm-project checkout here, or override `LLVM_PROJECT_DIR=`. |
| `patches/` | For local patches against the LLVM checkout (analogous to `amdgpu/patches/`). |
| `known-crashes/` | Hand-curated reproducers (empty for now). |

## Build

```
# 0. drop or symlink an llvm-project checkout
ln -s /path/to/llvm-project third_party/llvm-project

# 1. instrumented LLVM (slow; sancov + assertions + SPIRV;X86 targets)
./scripts/build_instrumented_llvm.sh

# 2. libFuzzer target
./scripts/build_directed_fuzzer.sh
```

To reuse an existing LLVM build instead of step 1, set
`LLVM_DIR=/path/to/llvm-build/lib/cmake/llvm` for step 2.  Coverage
feedback will be limited to the fuzzer TU (the harness), not the
backend, but the crash-detection path still works — that's how the
smoke run below was driven.

## Run

```
./scripts/run_directed_fuzzer.sh -runs=10000
```

Useful env vars (mirroring AMDGPU):

| Var | Default | Purpose |
| --- | --- | --- |
| `FUZZER_BIN` | `build/fuzzer/llvm_spirv_crash_fuzzer` | binary to run |
| `CORPUS_DIR` | `$FUZZX_RUNTIME_ROOT/corpus/directed` | libFuzzer corpus |
| `ARTIFACT_DIR` | `$FUZZX_RUNTIME_ROOT/artifacts/directed` | libFuzzer crash dumps |
| `FUZZX_FINDINGS_DIR` | `$FUZZX_RUNTIME_ROOT/findings` | `.bc` / `.ll` for each finding the harness catches |
| `FUZZX_RUNTIME_ROOT` | `${TMPDIR:-/tmp}/fuzzx-spirv-$USER` | parent for the above |

## Smoke run

A ~30-second run (`-runs=4000 -max_total_time=120`) against an
`llvm-project` build (LLVM 23.0.0git, assertions on, no sancov)
reproducibly hit:

```
Assertion `reservedRegsFrozen() && "Reserved registers haven't been frozen yet. "
                                   "Use TRI::getReservedRegs()."' failed.
  at llvm/include/llvm/CodeGen/MachineRegisterInfo.h:964
```

Caveat: the saved `.bc` does **not** reproduce under a standalone
`llc -mtriple=spirv64` invocation.  The crash only fires inside the
in-process harness, where `TargetMachine` instances (and any backend
global state) are shared across many compilations — same caching
pattern as AMDGPU's `getTargetMachine`.  That means it's either:

1. a real SPIR-V backend bug latent on repeated in-process codegen
   (something fails to re-freeze reserved regs between functions /
   modules when the TM is reused), or
2. a harness artifact from how this crash fuzzer drives the SPIR-V
   backend.

Distinguishing the two requires reducing further and trying to repro
with a small in-process driver that just loops `llc`-equivalent
codegen on the saved `.bc`.  Not done.

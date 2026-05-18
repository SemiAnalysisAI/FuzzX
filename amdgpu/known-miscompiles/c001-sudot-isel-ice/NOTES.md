# c001: `sudot4` and `sudot8` abort in AMDGPU instruction selection

Found while expanding the LLVM-bitcode C++ fuzzer to emit AMDGPU integer
dot-product intrinsics. The fuzzer first generated a larger program containing
`llvm.amdgcn.sudot8`; reducing showed that a single mixed signed/unsigned dot
intrinsic call is enough to crash instruction selection. A follow-up sweep
showed the same failure for `llvm.amdgcn.sudot4`.

```bash
known-miscompiles/run_ll_compiler_reproducer.sh \
  known-miscompiles/c001-sudot-isel-ice/reduced-sudot8.ll
known-miscompiles/run_ll_compiler_reproducer.sh \
  known-miscompiles/c001-sudot-isel-ice/reduced-sudot4.ll
```

Observed result on the ROCm 7.2.3 source build for both reproducers:

```text
O0=fail
O0-message=fatal error: error in backend: Cannot select: intrinsic %llvm.amdgcn.sudot8
O2=fail
O2-message=fatal error: error in backend: Cannot select: intrinsic %llvm.amdgcn.sudot8
compiler_failure=true
```

The `sudot4` reproducer is identical except that the message names
`%llvm.amdgcn.sudot4`.

## Reduction

The reduced `sudot8` kernel is:

```llvm
define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %out) #0 {
entry:
  %r = call i32 @llvm.amdgcn.sudot8(i1 true, i32 0, i1 true,
                                    i32 0, i32 0, i1 false)
  store i32 %r, ptr addrspace(1) %out, align 4
  ret void
}
```

The intrinsic declaration matches LLVM's AMDGPU intrinsic definition, and all
`immarg` operands are constants. The module targets `amdgcn-amd-amdhsa` with
`gfx950`, matching the fuzzer's default target CPU.

## Root Cause Notes

The backend aborts during `AMDGPU DAG->DAG Pattern Instruction Selection` with
`Cannot select: intrinsic %llvm.amdgcn.sudot4` or
`Cannot select: intrinsic %llvm.amdgcn.sudot8`. The signed-only and
unsigned-only forms tested during triage (`sdot2`, `udot2`, `sdot4`, `udot4`,
`sdot8`, and `udot8`) do select for the same target, so the uncovered case is
specific to the mixed signed/unsigned `sudot*` intrinsic family.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces at `-O0` and `-O2`: `Cannot select: intrinsic %llvm.amdgcn.sudot4` / `%llvm.amdgcn.sudot8`. |
| LLVM HEAD, commit `10756d32f96154f0889eda159ea9a26bc4188bda` | Reproduces at `-O0` and `-O2`: `Cannot select: intrinsic %llvm.amdgcn.sudot4` / `%llvm.amdgcn.sudot8`. |
| ROCm HEAD, commit `9115c466b3577830455f70c4f492429bf6c64b25` | Reproduces at `-O0` and `-O2`: `Cannot select: intrinsic %llvm.amdgcn.sudot4` / `%llvm.amdgcn.sudot8`. |

Original fuzzer input SHA-1:

```text
2cb2f9b968130dc4a520e6754a2c38c04bf98525
```

## Fuzzer Follow-Up

The IR-bitcode fuzzer now suppresses generated `llvm.amdgcn.sudot4` and
`llvm.amdgcn.sudot8` calls by default. Set
`FUZZX_ALLOW_C001_SUDOT_ISEL_ICE=1` to re-enable these intrinsics when
replaying or investigating the crash.

# c002: `llvm.amdgcn.fma.legacy` fails instruction selection at `-O0`

## Summary

AMDGPU `-O0` aborts during codegen when a `gfx950` kernel contains a dynamic
`llvm.amdgcn.fma.legacy` call. The same reduced IR compiles at `-O2`.

This was found while adding bounded AMDGPU FP intrinsic generation to the C++
directed fuzzer.

## Reproduce

From `amdgpu/`:

```bash
known-miscompiles/run_ll_compiler_reproducer.sh \
  known-miscompiles/c002-fma-legacy-isel-ice/reduced.ll
```

Observed result:

```text
O0=fail
O0-exit=1
O0-message=fatal error: error in backend: Cannot select: intrinsic %llvm.amdgcn.fma.legacy
O2=pass
compiler_failure=true
```

## Root Cause Notes

The reduced IR is defined: the loaded input is masked before `uitofp`, the
legacy FMA result is in `[3, 468]`, and the `fptoui` result is in range.

The failing `-O0` path leaves the target intrinsic for instruction selection,
where AMDGPU codegen reports that it cannot select
`llvm.amdgcn.fma.legacy`. The `-O2` pipeline compiles the same IR, apparently
because the optimized path rewrites or lowers the operation before it reaches
the failing selector path.

## Toolchain Results

Checked on 2026-05-18 on `gfx950`.

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | `-O0` fails, `-O2` passes. |
| LLVM HEAD, commit `10756d32f96154f0889eda159ea9a26bc4188bda` | `-O0` fails, `-O2` passes. |
| ROCm HEAD, commit `9115c466b3577830455f70c4f492429bf6c64b25` | `-O0` fails, `-O2` passes. |

## Fuzzer Follow-Up

The directed fuzzer suppresses `llvm.amdgcn.fma.legacy` by default. Set
`FUZZX_ALLOW_C002_FMA_LEGACY_ISEL_ICE=1` to re-enable this intrinsic.

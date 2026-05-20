# c006: `llvm.amdgcn.tanh` ICEs in AMDGPU instruction selection on gfx950

`v_tanh_f32` is a transcendental from the GFX12 (RDNA4) ISA -- it is not
present on CDNA targets including gfx950.  Like the other entries in the
c003--c005 family, the intrinsic is declared target-unconditionally, so
when it shows up in IR built for a CDNA target the SDAG matcher aborts
with `Cannot select` instead of giving a clean diagnostic.

```bash
known-miscompiles/run_ll_compiler_reproducer.sh \
  known-miscompiles/c006-tanh-isel-ice/reduced.ll
```

Observed output (LLVM HEAD with the five PR patches, target `gfx950`):

```text
O0=fail
O0-exit=1
O0-message=fatal error: error in backend: Cannot select: intrinsic %llvm.amdgcn.tanh
O2=fail
O2-exit=1
O2-message=fatal error: error in backend: Cannot select: intrinsic %llvm.amdgcn.tanh
compiler_failure=true
```

Both `tanh.f32` and `tanh.f16` reproduce; the entry only ships the f32
version because reduction is trivial.

The expected fix shape is the same as c003: gate the intrinsic on the
appropriate subtarget feature so clang/SDAG produces an "intrinsic not
supported on subtarget" diagnostic rather than crashing.  Contrast with
`llvm.amdgcn.rcp.legacy` / `rsq.legacy`, which already give that clean
diagnostic on this target.

## Fuzzer Suppression

Not yet wired up.  Add a `c006`-style suppressor in
`fuzzer/llvm_amdgpu_diff_fuzzer.cpp` to drop `Intrinsic::amdgcn_tanh` from
any IR generator that targets CDNA, mirroring c001--c005.

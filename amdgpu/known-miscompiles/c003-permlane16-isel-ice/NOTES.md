# c003: `llvm.amdgcn.permlane16` ICEs in AMDGPU instruction selection on CDNA targets

`v_permlane16_b32` is a GFX10+ (RDNA) cross-lane permutation instruction.  The
intrinsic is declared unconditionally in `IntrinsicsAMDGPU.td`, so it can be
called for any AMDGPU target -- including the CDNA / MI-series targets that
do not actually have the instruction.  When that happens, instruction
selection aborts with `Cannot select: intrinsic %llvm.amdgcn.permlane16`
instead of giving a clean diagnostic.

```bash
known-miscompiles/run_ll_compiler_reproducer.sh \
  known-miscompiles/c003-permlane16-isel-ice/reduced.ll
```

Observed output (LLVM HEAD with the five PR patches, `gfx950` selected via
the kernel's `target-cpu` attribute):

```text
O0=fail
O0-exit=1
O0-message=fatal error: error in backend: Cannot select: intrinsic %llvm.amdgcn.permlane16
O2=fail
O2-exit=1
O2-message=fatal error: error in backend: Cannot select: intrinsic %llvm.amdgcn.permlane16
compiler_failure=true
```

## Reduction

```llvm
define protected amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %out) #0 {
entry:
  %r = call i32 @llvm.amdgcn.permlane16.i32(i32 0, i32 0,
                                            i32 0, i32 0,
                                            i1 true, i1 true)
  store i32 %r, ptr addrspace(1) %out, align 4
  ret void
}

declare i32 @llvm.amdgcn.permlane16.i32(i32, i32, i32 immarg, i32 immarg, i1 immarg, i1 immarg)

attributes #0 = { convergent nounwind "target-cpu"="gfx950" }
```

## Target Sweep

I ran the same reduced IR for several AMDGPU targets to confirm the gating
issue is target-wide rather than gfx950-specific:

| Target | Result |
| --- | --- |
| `gfx900` | ICE: `Cannot select: intrinsic %llvm.amdgcn.permlane16` |
| `gfx906` | ICE |
| `gfx908` | ICE |
| `gfx90a` | ICE |
| `gfx942` | ICE |
| `gfx950` | ICE |
| `gfx1030` | OK (RDNA target, instruction supported) |

The CDNA family lacks `v_permlane16_b32`, so the proper behaviour is either
(a) a `clang` front-end diagnostic refusing the builtin for these targets, or
(b) a TableGen-level intrinsic predicate that emits a clean `LangErrorFn`
diagnostic.  Selecting and crashing is a backend bug.

`llvm.amdgcn.permlanex16` (the "extended" sibling) is almost certainly in the
same boat -- I didn't add a separate entry but a one-line addition to the
fuzzer suppressor that covers both names would prevent re-discovery.

## Fuzzer Suppression

Not yet wired up.  Add a `c003`-style suppressor in
`fuzzer/llvm_amdgpu_diff_fuzzer.cpp` to drop `Intrinsic::amdgcn_permlane16`
and `Intrinsic::amdgcn_permlanex16` from any IR generator that targets
CDNA, mirroring how c001 (`sudot4`/`sudot8`) and c002 (`fma.legacy`) are
gated off by default.

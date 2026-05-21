# c010: `STRICT_FP_EXTEND bf16 -> f32/f64` hits `llvm_unreachable` on gfx950

*Discovery method: code inspection (during STRICT_FP_* combiner audit).*

Sibling shape to c001/c003/c006/c008 (intrinsic without selector for
relevant target generation) and m143 (STRICT_FP_ROUND f64 -> bf16
silently drops chain).

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:4901-4917`
(`SITargetLowering::lowerFP_EXTEND`):

`STRICT_FP_EXTEND` is set Custom for f32/f64 dst at lines 580-581.
`lowerFP_EXTEND` detects bf16 source then hits `llvm_unreachable`
at lines 4914-4915 for the strict case:

```cpp
// FIXME: Need STRICT_BF16_TO_FP and/or strict expansion.
llvm_unreachable("Need STRICT_BF16_TO_FP");
```

Result: a release-asserts compiler crashes on any
`llvm.experimental.constrained.fpext.{f32,f64}.bf16`.

## Reproducer

`reduced.ll`:

```llvm
declare float @llvm.experimental.constrained.fpext.f32.bf16(bfloat, metadata)

define amdgpu_kernel void @t(ptr addrspace(1) %p, bfloat %x) #0 {
  %r = call float @llvm.experimental.constrained.fpext.f32.bf16(
         bfloat %x,
         metadata !"fpexcept.strict") #0
  store float %r, ptr addrspace(1) %p, align 4
  ret void
}

attributes #0 = { strictfp }
```

`llc -mtriple=amdgcn -mcpu=gfx950 -O0 reduced.ll`:

```
LLVM ERROR: Need STRICT_BF16_TO_FP
```

(or in a Release-with-Asserts build: assertion failure /
`llvm_unreachable` abort.)

## Suggested fix

1. Lower bf16 -> f32 strict as `STRICT_FP_EXTEND` via
   `bitcast bf16 -> i16; shl_i32 (zext i16, 16); bitcast i32 -> f32`,
   threading the strict chain through.  Or introduce
   `AMDGPUISD::STRICT_BF16_TO_FP` and a matching pattern.
2. As an immediate stop-gap, bail to the default expander when the
   opcode is strict so the legalizer can choose an unfused expansion
   that preserves chain.

Sibling pattern to:
* m143 (strict round bf16 dst drops chain)
* c008 (amdgcn.class.bf16 ICE)
* c003/c006 (intrinsic on wrong target ICE)

## Why the fuzzer hasn't caught it

* The IR fuzzer rarely emits `constrained.fpext` with bf16 source.
  Per `MEMORY.md` (Prefer-random-over-idioms), the random emitter
  should add constrained-extend intrinsics with all valid src
  type overloads.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | ICE at -O0 / -O2. |
| ROCm 7.1.1 | Same defect. |

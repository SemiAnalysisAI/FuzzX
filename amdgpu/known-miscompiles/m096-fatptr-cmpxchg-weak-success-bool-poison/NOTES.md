# m096: `AMDGPULowerBufferFatPointers` leaves weak `cmpxchg`'s success bool as poison

*Discovery method: code inspection.* Sibling shape to m085 (same file).

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPULowerBufferFatPointers.cpp:1881-1886`
(`SplitPtrStructs::visitAtomicCmpXchgInst`):

```cpp
Value *Res = PoisonValue::get(AI.getType());
Res = IRB.CreateInsertValue(Res, Call, 0);
if (!AI.isWeak()) {                                           // <-- BUG
  Value *Succeeded = IRB.CreateICmpEQ(Call, AI.getCompareOperand());
  Res = IRB.CreateInsertValue(Res, Succeeded, 1);
}
SplitUsers.insert(&AI);
AI.replaceAllUsesWith(Res);
```

A `cmpxchg` instruction -- weak or strong -- returns a `{T, i1}` where
the second field is the success indicator (per LangRef: "the original
value at location, paired with a flag indicating success (true) or
failure (false)").  The lowering only fills the success slot for
strong `cmpxchg`; for `cmpxchg weak` it leaves field 1 as `poison`.

The buffer hardware (`buffer_atomic_cmpswap`) is itself non-spurious,
so the same `ICmpEQ(Call, CompareOperand)` used in the strong path
is correct for the weak path too -- `weak` allows spurious failure but
does not require it; reporting "success iff loaded value == compare"
is a valid implementation for both forms.

Downstream the poison bool is consumed by `extractvalue ..., 1`, then
by branches / stores / arithmetics -- producing UB or undefined values.

## Reproducer

`reduced.ll`:

```llvm
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(7) %p,
                                       ptr addrspace(1) %out) {
entry:
  %r    = cmpxchg weak ptr addrspace(7) %p, i32 1, i32 2 monotonic monotonic
  %succ = extractvalue { i32, i1 } %r, 1
  %z    = zext i1 %succ to i32
  store i32 %z, ptr addrspace(1) %out, align 4
  ret void
}
```

`opt -passes=amdgpu-lower-buffer-fat-pointers -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950`
output (the buggy IR):

```llvm
%r    = call i32 @llvm.amdgcn.raw.ptr.buffer.atomic.cmpswap.i32(...)
%0    = insertvalue { i32, i1 } poison, i32 %r, 0     ; field 1 never set
%succ = extractvalue { i32, i1 } %0, 1                ; reads poison
%z    = zext i1 %succ to i32                          ; -> poison
store i32 %z, ptr addrspace(1) %out, align 4          ; stores poison
```

`llc -mcpu=gfx950 -O2` then emits `global_store_dword v6, v6, s[2:3]`
where `v6` is never initialized -- the asm shows
`; implicit-def: $vgpr4_vgpr5` around the `buffer_atomic_cmpswap` but
no init of `v6`.

A *strong* cmpxchg with the same shape correctly emits
`icmp eq i32 %r, 1` followed by `insertvalue ..., i1 %1, 1`, confirming
the asymmetry is conditional on `weak`.

## Why this matters in the default pipeline

`amdgpu-lower-buffer-fat-pointers` IS in the default `clang -O2`
codegen pipeline for `amdgcn-amd-amdhsa`.  Any front-end emitting a
`cmpxchg weak` on an `addrspace(7)` pointer (typical lock-free-loop
idioms compiled through HIP/OpenCL with buffer descriptors) will
branch on poison / store poison / arithmetic on poison.  Loops written
as `while (!success) { ... weak cmpxchg ...; success = res.second; }`
get undefined termination behaviour.

## How a fix should look

Drop the `if (!AI.isWeak())` guard:

```cpp
Value *Res = PoisonValue::get(AI.getType());
Res = IRB.CreateInsertValue(Res, Call, 0);
Value *Succeeded = IRB.CreateICmpEQ(Call, AI.getCompareOperand());
Res = IRB.CreateInsertValue(Res, Succeeded, 1);
```

The buffer atomic never spuriously fails, so "success iff loaded ==
expected" is a correct implementation for both `weak` and `strong`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (field 1 is `poison`). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/opt`) | Reproduces. |

Same demonstration approach as m085: this pass cannot be driven by
`run_ll_reproducer.sh` from HIP source because constructing real
`addrspace(7)` fat pointers at the language level is non-trivial, but
the bug is purely IR-level and the asm proves the poison reaches
codegen.

# m085: `AMDGPULowerBufferFatPointers` uses element store-size instead of alloc-size for array stride

*Discovery method: code inspection.*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPULowerBufferFatPointers.cpp:978-985` (`visitLoadImpl`):

```cpp
if (auto *AT = dyn_cast<ArrayType>(PartType)) {
  Type *ElemTy = AT->getElementType();
  if (!ElemTy->isSingleValueType() || !DL.typeSizeEqualsStoreSize(ElemTy) ||
      ElemTy->isVectorTy()) {
    TypeSize ElemStoreSize = DL.getTypeStoreSize(ElemTy);           // <-- BUG
    bool Changed = false;
    for (auto I : llvm::iota_range<uint32_t>(0, AT->getNumElements(), false)) {
      AggIdxs.push_back(I);
      Changed |= visitLoadImpl(OrigLI, ElemTy, AggIdxs,
                               AggByteOff + I * ElemStoreSize.getFixedValue(),
                               Result, Name + Twine(I));
```

LLVM lays out array elements at multiples of `getTypeAllocSize(ElemTy)`,
not `getTypeStoreSize(ElemTy)`.  The two only differ for types whose
ABI alignment forces padding past the natural store size -- e.g.
`<3 x i32>` under AMDGPU's data layout (`v96:128`) has `storeSize = 12`
but `allocSize = 16`.  The third disjunct of the gate
(`ElemTy->isVectorTy()`) deliberately routes *any* `[N x <K x T>]`
through this per-element splitter, and the +12 stride lands inside the
previous element's padding.

The symmetric bug is at lines 1098-1105 of the same file
(`visitStoreImpl`).

## Reproducer

`reduced.ll`:

```llvm
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(7) %p, ptr addrspace(1) %out) {
entry:
  %v   = load [2 x <3 x i32>], ptr addrspace(7) %p, align 16
  %e0  = extractvalue [2 x <3 x i32>] %v, 0
  %e1  = extractvalue [2 x <3 x i32>] %v, 1
  store <3 x i32> %e0, ptr addrspace(1) %out, align 16
  %e1p = getelementptr inbounds <3 x i32>, ptr addrspace(1) %out, i32 1
  store <3 x i32> %e1, ptr addrspace(1) %e1p, align 16
  ret void
}
```

Run:

```bash
/opt/rocm-7.1.1/lib/llvm/bin/opt -S \
    -passes=amdgpu-lower-buffer-fat-pointers \
    -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950 \
    amdgpu/known-miscompiles/m085-fatptr-array-vec-elem-store-vs-alloc-stride/reduced.ll
```

Key output:

```llvm
%v0.off.0     = call <3 x i32> @llvm.amdgcn.raw.ptr.buffer.load.v3i32(... %p.off, ...)
%p.off.ptr.12 = add nuw i32 %p.off, 12           ; <-- should be 16
%v1.off.12    = call <3 x i32> @llvm.amdgcn.raw.ptr.buffer.load.v3i32(... %p.off.ptr.12, ...)
```

`llc -mcpu=gfx950` then emits:

```
buffer_load_dwordx3 v[4:6],  v8,  s[4:7], 0 offen
buffer_load_dwordx3 v[8:10], v11, s[4:7], 0 offen offset:12   ; should be offset:16
```

So `%e1` is read from bytes `[12..24)` of `%p` -- it picks up the 4
bytes of padding after `%e0` and only the first 8 bytes of `%e1`.  The
symmetric store path overwrites `%e0`'s padding and short-stores `%e1`.

## Why this matters in the default pipeline

`amdgpu-lower-buffer-fat-pointers` *is* in the default `clang -O2`
codegen pipeline (visible in `-debug-pass=Structure` output for
`amdgcn-amd-amdhsa -mcpu=gfx950`).  Any source-level code that has an
`addrspace(7)` (buffer fat pointer) load/store of an array of vectors
whose element type is sub-128-bit (`<3 x i32>`, `<3 x float>`,
`<3 x i16>`, etc.) miscompiles.

## How a fix should look

Use `getTypeAllocSize(ElemTy)` for the per-element stride (matching how
SROA, the IR cloner, and SDAG legalization all space array elements):

```cpp
TypeSize ElemAllocSize = DL.getTypeAllocSize(ElemTy);
...
AggByteOff + I * ElemAllocSize.getFixedValue(),
```

Alternatively, dropping the `|| ElemTy->isVectorTy()` disjunct would
send `[N x vec]` through the typical-case flat-vector path below, which
already uses the correct alloc-size-aware stride implicitly.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (offset:12 instead of offset:16). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/opt`) | Reproduces. |
| ROCm 7.2.3 (source build) | Reproduces. |

The FuzzX harness cannot drive an end-to-end HIP test for this bug
because constructing a real `addrspace(7)` buffer fat pointer at the
source-language level is non-trivial, but the bug manifests purely at
IR + asm, so the `opt` / `llc` output above is the proof (same
demonstration approach as m083 and m084).

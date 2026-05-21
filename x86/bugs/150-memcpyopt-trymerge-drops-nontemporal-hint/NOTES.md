# MemCpyOpt tryMergingIntoMemset drops !nontemporal hint of merged-in stores

**Component:** llvm/lib/Transforms/Scalar/MemCpyOptimizer.cpp

**Function:** `MemCpyOptPass::tryMergingIntoMemset` (line 352)
(called from `processStore` line 790 and `processMemSet` line 832)

**Pattern:** Author intended to avoid `!nontemporal` loss for the *start*
store but did not extend the check to subsequent stores being merged.

## Bug

`processStore` explicitly bails when the start store has `!nontemporal`:
```cpp
// Avoid merging nontemporal stores since the resulting
// memcpy/memset would not be able to preserve the nontemporal hint.
if (SI->getMetadata(LLVMContext::MD_nontemporal))
  return false;
```
(line 749-756.)

But the forward-scan loop inside `tryMergingIntoMemset` (lines 396-427) only
checks `NextStore->isSimple()` — which is `!isAtomic() && !isVolatile()`,
and does **not** look at `!nontemporal`.  Any subsequent store in the merge
range can be nontemporal, and the resulting `Builder.CreateMemSet(...)`
(line 475) is plain memset with no nontemporal hint.

The hint is preserved for the start store (because `processStore` blocks
entry), but silently lost for every merged-in store.  The exact case the
author intended to avoid still happens — it's just gated on which store
happens to be the *first* in the run.

## Confirmed via opt

```ll
target triple = "x86_64-unknown-linux-gnu"

declare void @use(ptr)

define void @t(ptr %p) {
  %p0 = getelementptr i8, ptr %p, i64 0
  %p1 = getelementptr i8, ptr %p, i64 1
  %p2 = getelementptr i8, ptr %p, i64 2
  %p3 = getelementptr i8, ptr %p, i64 3
  store i8 0, ptr %p0
  store i8 0, ptr %p1, !nontemporal !0
  store i8 0, ptr %p2, !nontemporal !0
  store i8 0, ptr %p3, !nontemporal !0
  call void @use(ptr %p)
  ret void
}

!0 = !{i32 1}
```

After `opt -passes=memcpyopt -S`:

```ll
define void @t(ptr %p) {
  %p0 = getelementptr i8, ptr %p, i64 0
  %p1 = getelementptr i8, ptr %p, i64 1
  %p2 = getelementptr i8, ptr %p, i64 2
  %p3 = getelementptr i8, ptr %p, i64 3
  call void @llvm.memset.p0.i64(ptr align 1 %p0, i8 0, i64 4, i1 false)  ; no !nontemporal
  call void @use(ptr %p)
  ret void
}
```

The three `!nontemporal` stores were merged into a regular memset with no
nontemporal metadata.  X86 codegen will lower the merged memset to ordinary
mov/rep stos sequences instead of the requested MOVNT-class non-temporal
stores — defeating the explicit hardware-cache hint the user requested.

Per the comment in `processStore`, this exact behavior is what the bail
was supposed to prevent.

## Fix sketch

In `tryMergingIntoMemset`, when scanning subsequent stores, also reject any
that carry `!nontemporal`, mirroring the guard at the start store:

```cpp
if (NextStore->getMetadata(LLVMContext::MD_nontemporal))
  break;
```

For memsets in the merge loop (line 429), `MemSetInst` itself has no
`!nontemporal` form for the memset intrinsic, so no extra check needed there
beyond `MSI->isVolatile()`.

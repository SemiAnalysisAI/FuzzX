# w293 -- SROA `presplitLoadsAndStores` drops `!range`, `!noundef`, `!nontemporal`, `!invariant.load` on every split load

## Component
`llvm/lib/Transforms/Scalar/SROA.cpp`, `SROA::presplitLoadsAndStores`,
the split-load construction at lines 4840-4861.

```cpp
LoadInst *PLoad = IRB.CreateAlignedLoad(
    PartTy,
    getAdjustedPtr(IRB, DL, BasePtr,
                   APInt(DL.getIndexSizeInBits(AS), PartOffset),
                   PartPtrTy, BasePtr->getName() + "."),
    getAdjustedAlignment(LI, PartOffset),
    /*IsVolatile*/ false, LI->getName());
PLoad->copyMetadata(*LI, {LLVMContext::MD_mem_parallel_loop_access,
                          LLVMContext::MD_access_group});
```
(line 4840-4848)

## Root cause
Pre-splitting takes a wide integer load (say `load i64`) that crosses two
slice boundaries and rewrites it into N smaller loads (e.g. two `load i32`s).
The split loads inherit only `MD_mem_parallel_loop_access` and
`MD_access_group` from the original.

If the original load had any of:
- `!range` -- value range bounds (could be split into per-part subranges by
  `copyRangeMetadata` in Local.cpp:3119+)
- `!nontemporal` -- streaming load hint
- `!invariant.load`, `!invariant.group`, `!noundef`
- `!nonnull`, `!align`, `!dereferenceable` (when the part is also a pointer)
- `!tbaa_struct` and similar AA-related metadata

they are silently dropped on every split sub-load.

The store-side counterpart at line 4905 also copies only
`MD_mem_parallel_loop_access`, `MD_access_group`, and `MD_DIAssignID`, so
`!nontemporal` and any other store metadata is dropped on split stores too
(AA tags are handled separately via `adjustForAccess` at line 4909-4911).

## Reproducer
```ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define void @copy(ptr noalias %dst, i32 %lo, i32 %hi) {
entry:
  %a = alloca [8 x i8], align 8
  store i32 %lo, ptr %a, align 8
  %p = getelementptr i8, ptr %a, i64 4
  store i32 %hi, ptr %p, align 4
  ; i64 load over two i32 slices -> presplitLoadsAndStores triggers
  %v = load i64, ptr %a, align 8, !nontemporal !0, !range !1, !noundef !2
  store i64 %v, ptr %dst, align 8, !nontemporal !0
  ret void
}

!0 = !{i32 1}
!1 = !{i64 0, i64 65536}
!2 = !{}
```

`opt -passes=sroa -S`:
```ll
define void @copy(ptr noalias %dst, i32 %lo, i32 %hi) {
entry:
  %0 = zext i32 %hi to i64
  %1 = shl i64 %0, 32
  %2 = zext i32 %lo to i64
  %3 = or i64 %1, %2
  store i64 %3, ptr %dst, align 8                ; <-- !nontemporal gone
  ret void
}
```

The big load is split, then combined into the final `or` chain. The
`!nontemporal !0` and `!range !1` and `!noundef !2` are all gone. The
`!nontemporal` on the *store of %v* is also dropped (this is w292's path
for the store side -- shows the same root cause; not coincidence).

A clearer reproducer that keeps the split load visible:
```ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @sink(i64)
define void @copy(ptr noalias %src, ptr noalias %d0, ptr noalias %d1) {
entry:
  %a = alloca [8 x i8], align 8
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %a, ptr align 8 %src, i64 8, i1 false)
  ; small loads first so presplitting kicks in
  %r0 = load i32, ptr %a, align 8
  store i32 %r0, ptr %d0, align 4
  %p = getelementptr i8, ptr %a, i64 4
  %r1 = load i32, ptr %p, align 4
  store i32 %r1, ptr %d1, align 4
  ; finally the big load with metadata
  %v = load i64, ptr %a, align 8, !nontemporal !0, !range !1
  call void @sink(i64 %v)
  ret void
}
declare void @llvm.memcpy.p0.p0.i64(ptr nocapture writeonly, ptr nocapture readonly, i64, i1 immarg)
!0 = !{i32 1}
!1 = !{i64 0, i64 65536}
```

## Impact
- `!nontemporal` codegen regression on every split load -- the streaming hint
  is lost on each part. Affects HPC/ML/video pipelines that use wide
  streaming loads from a stack buffer.
- `!range` loss prevents downstream range-based folding on each part.
- `!invariant.load` loss can prevent hoisting that depended on the freshness
  guarantee.

## Fix sketch
- Use `copyMetadataForLoad(*PLoad, *LI)` (`Transforms/Utils/Local.h:434`)
  instead of the bare `copyMetadata({mem_parallel_loop_access, access_group})`
  call at SROA.cpp:4847.
- For `!range`, the helper already shells out to `copyRangeMetadata` which
  understands type changes (i64 -> i32 part).
- For split stores at line 4905 add `LLVMContext::MD_nontemporal` and any
  other store-safe metadata to the whitelist.

## Notes
- Default x86 -O2 only. Confirmed on LLVM 23.0.0git (FuzzX `opt` build).
- Distinct from w291 (single-load slice rewrite path in `visitLoadInst`,
  same file but different function and lines) and w292 (store-side analog
  for the simple visitStoreInst path).

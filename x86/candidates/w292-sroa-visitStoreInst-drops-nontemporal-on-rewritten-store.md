# w292 -- SROA `visitStoreInst` (all rewrite paths) drops `!nontemporal` on the rewritten store

## Component
`llvm/lib/Transforms/Scalar/SROA.cpp`:
- `AllocaSliceRewriter::visitStoreInst` adjusted-ptr arm at line 3366-3367
- `rewriteIntegerStore` at line 3304-3305
- `rewriteVectorizedStoreInst` at line 3276-3277
- `presplitLoadsAndStores` split-store at line 4905-4907

All four store rewrite paths use the same `copyMetadata` call:

```cpp
NewSI->copyMetadata(SI, {LLVMContext::MD_mem_parallel_loop_access,
                         LLVMContext::MD_access_group});
```
(SROA.cpp:3366, 3304, 3276; presplit at 4905 also adds `MD_DIAssignID`.)

## Root cause
The whitelist contains only loop-related metadata. **`!nontemporal` is dropped
on every SROA-rewritten store.** There is no codepath in SROA.cpp that copies
`MD_nontemporal` from the source store to the new store.

This is the symmetric problem to w291 on the load side. Stores can also
carry `!noundef` and `!nontemporal` (LangRef stores: `!nontemporal !N`
generates non-temporal stores -- `MOVNTDQ`/`VMOVNTDQ` on x86).

## Reproducer
```ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @consume(i32, i32)

define void @test(i32 %x, i32 %y) {
entry:
  %a = alloca [8 x i8], align 8
  store i32 %x, ptr %a, align 8, !nontemporal !0
  %p2 = getelementptr i8, ptr %a, i64 2
  store i32 %y, ptr %p2, align 2, !nontemporal !0
  %p4 = getelementptr i8, ptr %a, i64 4
  %r1 = load i32, ptr %a, align 8
  %r2 = load i32, ptr %p4, align 4
  call void @consume(i32 %r1, i32 %r2)
  ret void
}

!0 = !{i32 1}
```

`opt -passes=sroa -S`:
```ll
define void @test(i32 %x, i32 %y) {
entry:
  %a.sroa.0 = alloca i64, align 8
  store i32 %x, ptr %a.sroa.0, align 8                                 ; <-- !nontemporal gone
  %a.sroa.0.2.p2.sroa_idx1 = getelementptr inbounds i8, ptr %a.sroa.0, i64 2
  store i32 %y, ptr %a.sroa.0.2.p2.sroa_idx1, align 2                  ; <-- !nontemporal gone
  %a.sroa.0.0.a.sroa.0.0.r1 = load i32, ptr %a.sroa.0, align 8
  %a.sroa.0.4.p2.sroa_idx2 = getelementptr inbounds i8, ptr %a.sroa.0, i64 4
  %a.sroa.0.4.a.sroa.0.4.r2 = load i32, ptr %a.sroa.0.4.p2.sroa_idx2, align 4
  call void @consume(i32 %a.sroa.0.0.a.sroa.0.0.r1, i32 %a.sroa.0.4.a.sroa.0.4.r2)
  ret void
}
```

Both stores lose `!nontemporal`. The alloca was retained (still backed by a
real i64 alloca because the load-i32-at-offset-0 + store-i32-at-offset-2
pattern prevents full promotion), yet the nontemporal hint is lost.

## Impact
- Codegen regression: x86 lowering of `!nontemporal` stores selects
  `MOVNTDQ`/`MOVNTI`. Without the metadata the back-end emits a regular store
  that pollutes cache and changes machine code.
- This affects HPC, video, ML kernels that explicitly mark large streaming
  stores with `!nontemporal`.

## Fix sketch
Extend the metadata whitelist in all four store-rewrite paths to include at
least `LLVMContext::MD_nontemporal`, and ideally also use a helper analogous
to `copyMetadataForLoad` but for stores (which would also carry `!noundef`,
`!noalias`, `!alias.scope`, etc. where appropriate).

## Notes
- Default x86 -O2 only. Confirmed on LLVM 23.0.0git (FuzzX `opt` build).
- Distinct from w61/w78 (atomic flag drop). Distinct from w111
  (SimplifyCFG mergeCondStores drops `!nontemporal` -- different pass,
  different file).
- Distinct from w291 which is the load-side analog in the same SROA file.

# w209: CodeGenPrepare::splitMergedValStore drops !mem_parallel_loop_access and !access_group

**File:** `llvm/lib/CodeGen/CodeGenPrepare.cpp`
**Lines:** 8568-8662 (the `CreateSplitStore` lambda around lines 8638-8654)
**Function:** `splitMergedValStore`

## Summary
`splitMergedValStore` splits a wide store (`store (or (zext lo to i64), (shl (zext hi to i64), 32)), addr`) into two narrower stores via `Builder.CreateAlignedStore(V, Addr, Alignment)`. The IRBuilder factory does not propagate any metadata from the original `SI`. Bug #140 in this repo already documents the loss of `!nontemporal`, `!tbaa`, `!alias.scope`, `!noalias`, `!DIAssignID`, and `!annotation`. This candidate documents two additional loss categories that are essential for loop-parallelism analysis: `!mem_parallel_loop_access` and `!access_group` (the LoopVectorizer/loop-aware-passes metadata declaring that the access participates in a parallel iteration).

If a later loop pass (running after CGP) sees a parallel access broken into two stores where the metadata is lost, it can no longer prove the parallel-iteration property and either declines a vectorization that the source declared valid, or worse, mishandles the loop in a way that depends on which store has metadata and which does not.

## Source

```c++
auto CreateSplitStore = [&](Value *V, bool Upper) {
  V = Builder.CreateZExtOrBitCast(V, SplitStoreType);
  Value *Addr = SI.getPointerOperand();
  Align Alignment = SI.getAlign();
  const bool IsOffsetStore = (IsLE && Upper) || (!IsLE && !Upper);
  if (IsOffsetStore) {
    Addr = Builder.CreateGEP(
        SplitStoreType, Addr,
        ConstantInt::get(Type::getInt32Ty(SI.getContext()), 1));
    Alignment = commonAlignment(Alignment, HalfValBitSize / 8);
  }
  Builder.CreateAlignedStore(V, Addr, Alignment);  // <-- no metadata copy
};
```

There is no call to `setMetadata(LLVMContext::MD_mem_parallel_loop_access, ...)` or `setMetadata(LLVMContext::MD_access_group, ...)`. These metadata kinds are *combinable* (see `Instruction::combineMetadataForCSE` / `Instruction::getAllMetadataOtherThanDebugLoc` policies for memory-access merging), so the right fix is to call `NewStore->copyMetadata(SI, {LLVMContext::MD_mem_parallel_loop_access, LLVMContext::MD_access_group, ...all other store-applicable kinds})`.

## Reproducer (`test_splitstore_pla.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

define void @test(ptr %p, float %f, i32 %i) {
entry:
  br label %loop
loop:
  %iv = phi i32 [ 0, %entry ], [ %iv.next, %loop ]
  %bf = bitcast float %f to i32
  %zlo = zext i32 %bf to i64
  %zhi = zext i32 %i to i64
  %hi.shl = shl i64 %zhi, 32
  %merged = or i64 %hi.shl, %zlo
  store i64 %merged, ptr %p, align 8, !mem_parallel_loop_access !1, !access_group !2
  %iv.next = add i32 %iv, 1
  %cmp = icmp slt i32 %iv.next, 10
  br i1 %cmp, label %loop, label %exit, !llvm.loop !0
exit:
  ret void
}

!0 = distinct !{!0, !{!"llvm.loop.parallel_accesses", !2}}
!1 = !{!0}
!2 = distinct !{}
```

x86's `isMultiStoresCheaperThanBitsMerge` (`X86ISelLowering.h:241`) returns true only when the lo/hi types are a float+int mix, so we use a `bitcast float %f to i32` for the low half.

## Reproduce
```
$ /home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc \
    -mtriple=x86_64-unknown-linux-gnu -stop-after=codegenprepare \
    test_splitstore_pla.ll -o -
```

Observed IR after CGP:
```llvm
store i32 %bf, ptr %p, align 8
%0 = getelementptr i32, ptr %p, i32 1
store i32 %i, ptr %0, align 4
```

Both split stores have lost `!mem_parallel_loop_access !1` and `!access_group !2`. The original store carried both.

## Suggested fix
After `Builder.CreateAlignedStore(...)`, copy the surviving-applicable metadata from `SI`. The full set should match what `splitMergedValStore`-equivalent splits would preserve for any store:
```c++
auto *NS = Builder.CreateAlignedStore(V, Addr, Alignment);
NS->copyMetadata(SI, {
    LLVMContext::MD_tbaa, LLVMContext::MD_alias_scope,
    LLVMContext::MD_noalias, LLVMContext::MD_nontemporal,
    LLVMContext::MD_mem_parallel_loop_access, LLVMContext::MD_access_group,
    LLVMContext::MD_DIAssignID, LLVMContext::MD_annotation});
```

## Impact
Loss of parallel-loop-access guarantees that the source declared. Downstream loop-aware passes (loop-vectorizer cleanup, scalar evolution-based memory analyses, llvm.loop.parallel_accesses-driven heuristics) can no longer recognize the split stores as part of the parallel iteration.

## Relation to bug #140
This candidate extends bug #140 (`splitMergedValStore` drops nontemporal/tbaa/alias-scope/noalias/DIAssignID/annotation). The same root cause — `Builder.CreateAlignedStore` doesn't carry metadata — also drops `!mem_parallel_loop_access` and `!access_group`. The fix for #140 should be widened to include these two loop-parallelism metadata kinds, or, more robustly, all `Instruction::getAllMetadata`-iterable kinds that apply to stores.

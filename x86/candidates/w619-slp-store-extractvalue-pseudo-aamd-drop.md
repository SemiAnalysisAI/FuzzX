# w619: SLP — ExtractValue path strips all AAMD; cumulative effect with w618

## Class
Info-loss / AA regression — *root cause sibling of w618*, but distinct in
that this examines the cumulative effect when an aggregate-returning call
(`call {i32, i32, i32, i32}`) feeds SLP through the
`Instruction::ExtractValue` code path with non-trivial alias metadata
attached to the operation.

## Component
`llvm/lib/Transforms/Vectorize/SLPVectorizer.cpp:22693-22702`
(same site as w618)

## Source

```cpp
// SLPVectorizer.cpp:22693
case Instruction::ExtractValue: {
  auto *LI = cast<LoadInst>(E->getSingleOperand(0));
  Builder.SetInsertPoint(LI);
  Value *Ptr = LI->getPointerOperand();
  LoadInst *V = Builder.CreateAlignedLoad(VecTy, Ptr, LI->getAlign());
  Value *NewV = ::propagateMetadata(V, E->Scalars);
  …
}
```

The bug is that `propagateMetadata(V, E->Scalars)` consults the
ExtractValue scalars instead of `LI`. The downstream effect is that even
`!noalias` scopes that the user (or `__restrict`/`noalias` parameter
inference) carefully attached to `LI` are silently destroyed. AA cannot
recover the lost scope set in any downstream pass.

## Repro

```ll
; w619.ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

%struct4 = type { i32, i32, i32, i32 }

declare void @ext(ptr %p) memory(read)

define void @t(ptr noalias %src, ptr noalias %dst) {
entry:
  %v = load %struct4, ptr %src, align 4, !alias.scope !0, !noalias !1, !tbaa !2
  call void @ext(ptr %dst)
  %x0 = extractvalue %struct4 %v, 0
  %x1 = extractvalue %struct4 %v, 1
  %x2 = extractvalue %struct4 %v, 2
  %x3 = extractvalue %struct4 %v, 3
  %q0 = getelementptr inbounds i32, ptr %dst, i64 0
  %q1 = getelementptr inbounds i32, ptr %dst, i64 1
  %q2 = getelementptr inbounds i32, ptr %dst, i64 2
  %q3 = getelementptr inbounds i32, ptr %dst, i64 3
  store i32 %x0, ptr %q0, align 4
  store i32 %x1, ptr %q1, align 4
  store i32 %x2, ptr %q2, align 4
  store i32 %x3, ptr %q3, align 4
  ret void
}

!0 = !{!3}
!1 = !{!4}
!2 = !{!5, !5, i64 0}
!3 = !{!"scope_src", !6}
!4 = !{!"scope_dst", !6}
!5 = !{!"int", !7, i64 0}
!6 = !{!"domain"}
!7 = !{!"omnipotent char", !8, i64 0}
!8 = !{!"Simple C++ TBAA"}
```

After `opt -passes=slp-vectorizer -S`:
- `<4 x i32>` load is emitted at the position of the original aggregate
  load
- It has **no** `!alias.scope`, `!noalias`, or `!tbaa`
- Hoisting / sinking opportunities relying on the scope distinction
  between src and dst are gone

## Why this is worth a separate report
w618 documents the wrong-VL passing and the resulting `!tbaa` loss in a
straight-line example. w619 demonstrates the same defect causes loss of
the alias scope set / noalias scope set in a more realistic case that
also has a clobbering call between the load and the extractvalues — the
scenario where AAMD actually changes hoisting decisions.

## Fix
Same one-liner as w618: build `LoadVL = {LI}` and pass it.

## Triage
Latent since SLP's ExtractValue handling was introduced.

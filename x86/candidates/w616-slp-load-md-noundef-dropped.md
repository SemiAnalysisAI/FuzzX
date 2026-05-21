# w616: SLP combined integer loads drop `!noundef` and `!range`

## Class
Info-loss (not miscompile). Same allowlist defect as w615, but specifically
demonstrated for value-domain attributes on integer loads.

## Component
`llvm/lib/Transforms/Vectorize/SLPVectorizer.cpp:23238` →
`llvm/lib/Analysis/VectorUtils.cpp:1049-1068` (`getMetadataToPropagate`
allowlist excludes `MD_range` and `MD_noundef`).

## Source

See w615 for the allowlist excerpt. Per the SLP load emission site:

```cpp
// SLPVectorizer.cpp:23236
Value *V = E->State == TreeEntry::CompressVectorize
               ? NewLI
               : ::propagateMetadata(NewLI, E->Scalars);
```

The call goes through `llvm::propagateMetadata` which in turn calls
`getMetadataToPropagate` — that strips every metadata kind not on its hard-coded
allowlist of 8 IDs. `!noundef` (`MD_noundef`) and `!range` (`MD_range`) are
both absent.

## Repro

```ll
; w616.ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define void @load4_md(ptr noalias %src, ptr noalias %dst) {
entry:
  %p0 = getelementptr inbounds i32, ptr %src, i64 0
  %p1 = getelementptr inbounds i32, ptr %src, i64 1
  %p2 = getelementptr inbounds i32, ptr %src, i64 2
  %p3 = getelementptr inbounds i32, ptr %src, i64 3
  %l0 = load i32, ptr %p0, align 4, !range !0, !noundef !1
  %l1 = load i32, ptr %p1, align 4, !range !0, !noundef !1
  %l2 = load i32, ptr %p2, align 4, !range !0, !noundef !1
  %l3 = load i32, ptr %p3, align 4, !range !0, !noundef !1
  %q0 = getelementptr inbounds i32, ptr %dst, i64 0
  %q1 = getelementptr inbounds i32, ptr %dst, i64 1
  %q2 = getelementptr inbounds i32, ptr %dst, i64 2
  %q3 = getelementptr inbounds i32, ptr %dst, i64 3
  store i32 %l0, ptr %q0, align 4
  store i32 %l1, ptr %q1, align 4
  store i32 %l2, ptr %q2, align 4
  store i32 %l3, ptr %q3, align 4
  ret void
}

!0 = !{i32 0, i32 100}
!1 = !{}
```

After `opt -passes=slp-vectorizer -S`:

```
  %0 = load <4 x i32>, ptr %p0, align 4    ; !range & !noundef gone
  store <4 x i32> %0, ptr %q0, align 4
```

Downstream `KnownBits` (CVP, InstCombine simplifyDemandedBits) and freeze
elimination lose the ability to use the [0,100) range or the noundef property
that the scalar code provides for free.

## Severity / Triage
Same fix as w615 — single allowlist edit in
`llvm/lib/Analysis/VectorUtils.cpp`. Existing combiners (`MDNode::intersect`
for `!noundef`, `getMostGenericRange` for `!range`) already exist in
`llvm/include/llvm/IR/Metadata.h`. The bug has been latent since `!noundef`
was introduced. Non-miscompile; pure optimization regression on
vectorization-heavy code.

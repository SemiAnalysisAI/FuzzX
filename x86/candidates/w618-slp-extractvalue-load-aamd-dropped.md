# w618: SLP ExtractValue-of-load lowering drops AAMD from the underlying load

## Class
Info-loss / **alias-analysis regression**. The combined vector load created
when SLP widens `extractvalue %struct, i` patterns is emitted with the AAMD
of the *ExtractValue* scalars (which carry none), not the AAMD of the
*LoadInst* whose value they are extracting from. TBAA, alias scope, noalias,
and !nontemporal silently vanish.

## Component
`llvm/lib/Transforms/Vectorize/SLPVectorizer.cpp:22693-22702`

## Source

```cpp
// SLPVectorizer.cpp:22693
case Instruction::ExtractValue: {
  auto *LI = cast<LoadInst>(E->getSingleOperand(0));     // <-- the real load
  Builder.SetInsertPoint(LI);
  Value *Ptr = LI->getPointerOperand();
  LoadInst *V = Builder.CreateAlignedLoad(VecTy, Ptr, LI->getAlign());
  Value *NewV = ::propagateMetadata(V, E->Scalars);      // <-- BUG: E->Scalars are ExtractValues
  NewV = FinalShuffle(NewV, E);
  E->VectorizedValue = NewV;
  return NewV;
}
```

`E->Scalars` are the `extractvalue` instructions. They typically carry no
metadata. `propagateMetadata` therefore collects only the (empty) AAMD set
from `E->Scalars[0]` and produces a vector load with no AAMD. The original
`LI`'s `!tbaa`, `!noalias`, `!alias.scope`, `!nontemporal`, `!access_group`,
`!mmra`, `!invariant.load` are all lost.

Correct behavior would be to propagate metadata from `{LI}` (or from
`{LI, LI, …, LI}` so that any per-extract-value enrichment also gets
intersected in). Cf. neighbouring cases (`Instruction::Load` at 23131,
`Instruction::Store` at 23247) which call `propagateMetadata` on
`E->Scalars` because those `E->Scalars` *are* the loads/stores carrying
the metadata.

## Repro

```ll
; w618.ll  --  /tmp/slphunt/extractval.ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

%struct4 = type { i32, i32, i32, i32 }

define void @t(ptr noalias %src, ptr noalias %dst) {
entry:
  %v  = load %struct4, ptr %src, align 4, !tbaa !0
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
!0 = !{!1, !1, i64 0}
!1 = !{!"int", !2, i64 0}
!2 = !{!"omnipotent char", !3, i64 0}
!3 = !{!"Simple C++ TBAA"}
```

`opt -passes=slp-vectorizer -S w618.ll` emits:

```
  %0 = load <4 x i32>, ptr %src, align 4    ; !tbaa LOST
  store <4 x i32> %0, ptr %q0, align 4
```

The original `!tbaa !0` is gone. Subsequent AA queries lose the int-TBAA
discrimination, blocking hoisting / sinking across calls with conflicting
TBAA, and forfeiting opt opportunities GVN/MemorySSA could otherwise exploit.

## Why this matters
Unlike w615-w617 which complain about pointer- and value-attribute metadata
the allowlist filters out, this bug is in the SLP *caller* — it passes the
wrong `VL` to `propagateMetadata`. Even if the allowlist were perfect, the
correct AAMD source (the underlying `LoadInst`) is not consulted. So
extending the allowlist (the w615-w617 fix) does NOT fix this site.

## Suggested fix

```cpp
SmallVector<Value *, 1> LoadVL{LI};
Value *NewV = ::propagateMetadata(V, LoadVL);
```

(or, if multiple distinct underlying loads can drive a single ExtractValue
bundle in future, walk `E->Scalars` extracting `getSingleOperand(0)` and
deduplicate.)

## Severity
Low for correctness (AA-drop is conservative), Medium for perf on
struct-returning calls / GEP-of-aggregate hot paths (libc++ `tuple`,
`pair`, etc.).

# LoopUnroll `loadCSE` drops `!align` metadata, demoting alignment guarantees

## File and root cause

Same call site as w330: `llvm/lib/Transforms/Utils/LoopUnroll.cpp`,
`loadCSE` at lines 277-340 (specifically lines 314-322). The bare
`replaceAllUsesWith` / `eraseFromParent` does no metadata combining, so
**any** load-side metadata that lives only on the eliminated load is gone
after CSE.

This filing is a focused variant of w330 (which covers `!nontemporal`)
that captures a different observable consequence: a *loaded pointer
alignment hint* (`!align`) is silently demoted when the two loads are
CSEd inside the unrolled body. Unlike a pure performance hint, `!align`
expresses an actual semantic guarantee that downstream passes (vector
load formation, byte-aligned codegen, the back end's alignment-derived
folding) can rely on. Losing it is not a perf hint loss — it's a missed
guarantee that further optimization could have exploited.

```cpp
// LoopUnroll.cpp:314-322 — loadCSE eliminates Load but no MD merge.
if (Value *M =
        getMatchingValue(LV, Load, CurrentGeneration, BAA, GetMSSA)) {
  if (LI.replacementPreservesLCSSAForm(Load, M)) {
    Load->replaceAllUsesWith(M);          // <-- no combineMetadataForCSE
    Load->eraseFromParent();
  }
} else {
  AvailableLoads.insert(PtrSCEV, LoadValue(Load, CurrentGeneration));
}
```

The `!align` MD on a load tells the optimizer "the loaded pointer value
is aligned to N bytes." When two loads of `ptr` produce the same value
(same address, no intervening write) and one is annotated `!align 16`
while the other isn't, the union of the two pieces of information is
"the loaded value is `!align 16`." A correctly merging CSE would keep
this and place it on the surviving load. LoopUnroll's CSE just keeps
whatever was already on the leader.

## Reproducer

`/tmp/unroll-test/test-loadcse-invariant.ll`:

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define ptr @align_loss(ptr noalias %pp, i32 %n) {
entry:
  br label %loop

loop:
  %i   = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  %gep = getelementptr ptr, ptr %pp, i32 %i
  %a   = load ptr, ptr %gep                       ; regular -> leader
  %b   = load ptr, ptr %gep, !align !1            ; 16-byte aligned -> eliminated
  call void @use(ptr %a)
  call void @use(ptr %b)
  %i.next = add nuw i32 %i, 1
  %cmp    = icmp ult i32 %i.next, %n
  br i1 %cmp, label %loop, label %exit

exit:
  ret ptr %b
}

declare void @use(ptr)

!1 = !{i64 16}
```

### `opt -passes=loop-unroll -unroll-count=2 -unroll-allow-partial -S`

```llvm
loop:
  %gep = getelementptr ptr, ptr %pp, i32 %i
  %a   = load ptr, ptr %gep, align 8            ; NO !align !1
  call void @use(ptr %a)
  call void @use(ptr %a)                        ; was use(ptr %b)
  %i.next = add nuw nsw i32 %i, 1
  %cmp = icmp ult i32 %i.next, %n
  br i1 %cmp, label %loop.1, label %exit

loop.1:
  %gep.1 = getelementptr ptr, ptr %pp, i32 %i.next
  %a.1   = load ptr, ptr %gep.1, align 8        ; NO !align !1 either
  call void @use(ptr %a.1)
  call void @use(ptr %a.1)
  %i.next.1 = add nuw i32 %i, 2
  %cmp.1 = icmp ult i32 %i.next.1, %n
  br i1 %cmp.1, label %loop, label %exit, !llvm.loop !0
```

The `!align !{i64 16}` from the eliminated `%b` is gone. Every subsequent
use of the loaded pointer downstream is now seen as having only the
default natural pointer alignment.

## Why this is a regression

`!align` is consumed by:

* `InstCombine` / `simplifyAlignmentAssumption` to fold away alignment
  checks on the loaded pointer.
* `LoopVectorize`/`SLPVectorizer` when deciding whether the loaded
  pointer is safe to use as the base of an aligned vector load.
* The x86 back end's selection of aligned vs unaligned vector moves
  (`MOVAPS` vs `MOVUPS`, `VMOVDQA` vs `VMOVDQU`) for pointers loaded out
  of indirection tables.
* `Attributor`/IPSCCP for pointer alignment derivation.

Dropping the hint demotes the loaded pointer to "natural alignment only"
and prevents these passes from generating the aligned form they could
have. On x86 -O2 with vector code that loads function-pointer or
data-pointer tables out of an indirection array, this can flip a back
end's choice from a single aligned move to an unaligned move sequence.

## Same fix as w330

Replace the bare RAUW with the standard metadata-merging CSE helper:

```cpp
if (LI.replacementPreservesLCSSAForm(Load, M)) {
  if (auto *MI = dyn_cast<Instruction>(M))
    combineMetadataForCSE(MI, Load, /*DoesKMove=*/false);
  Load->replaceAllUsesWith(M);
  Load->eraseFromParent();
}
```

`combineMetadataForCSE` handles `!align` correctly (it picks the
*larger* alignment of the two — the union of the two pieces of
information rather than the intersection — which is the semantically
correct merge for an alignment guarantee).

## Related

* w330: same root cause, focuses on `!nontemporal` (perf hint).
* `Local.cpp::combineMetadataForCSE` is the canonical helper; every CSE
  site in tree calls it.

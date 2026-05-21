# LoopUnroll `loadCSE` drops `!nontemporal` (and other) metadata

## File and root cause

`llvm/lib/Transforms/Utils/LoopUnroll.cpp` — function `loadCSE` (lines 277-340),
called from `simplifyLoopAfterUnroll` (line 369). The redundant-load
elimination uses a raw `replaceAllUsesWith`/`eraseFromParent` and never merges
metadata:

```cpp
// LoopUnroll.cpp:312-322
const SCEV *PtrSCEV = SE.getSCEV(Load->getPointerOperand());
LoadValue LV = AvailableLoads.lookup(PtrSCEV);
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

Compare to every other CSE site in the tree (EarlyCSE, GVN, NewGVN,
`Local.cpp::combineMetadataForCSE`): when one load is removed because a
prior load at the same address is available, the *surviving* load receives
the intersection of metadata. `loadCSE` here intersects nothing — the
surviving leader keeps exactly the metadata it had on entry, and any
metadata that lived **only** on the eliminated load is silently dropped.

In particular, `!nontemporal` is asymmetric: if the eliminated load was the
non-temporal one but the leader was a regular load, after RAUW the
non-temporal hint is gone. The same applies to `!align`, `!range`
(possibly tighter on the dropped load), `!invariant.load`, and friends.

This CSE only fires because unrolling has just produced syntactically
distinct loads from the same SCEV expression across iterations — the
duplicates did not exist in the input IR, so `EarlyCSE` never had a chance
to cover them before the loop got unrolled, and the bug is unique to this
intra-unroll CSE.

## Reproducer: `!nontemporal` lost across iterations

`/tmp/unroll-test/test-loadcse-post-unroll.ll`:

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i32 @cse_after_unroll(ptr noalias %p, i32 %n) {
entry:
  br label %loop

loop:
  %i        = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  %sum      = phi i32 [ 0, %entry ], [ %add2,   %loop ]
  %gep1     = getelementptr i32, ptr %p, i32 %i
  %i.plus1  = add i32 %i, 1
  %gep2     = getelementptr i32, ptr %p, i32 %i.plus1
  %a        = load i32, ptr %gep1, !nontemporal !0   ; NT load at p[i]
  %b        = load i32, ptr %gep2                    ; regular load at p[i+1]
  %add1     = add i32 %sum,  %a
  %add2     = add i32 %add1, %b
  %i.next   = add nuw i32 %i, 1
  %cmp      = icmp ult i32 %i.next, %n
  br i1 %cmp, label %loop, label %exit

exit:
  ret i32 %add2
}

!0 = !{i32 1}
```

### `opt -passes=loop-unroll -unroll-count=2 -unroll-allow-partial -S`

After unrolling, in `loop.1` (the second cloned iteration) the would-be NT
load of `p[i+1]` is removed because `loadCSE` found an earlier load at the
same SCEV (the regular `%b = load p[%i.plus1]` from the first iteration).
The `%add1.1` user is RAUWed to that regular `%b`:

```llvm
loop:
  ...
  %a       = load i32, ptr %gep1, align 4, !nontemporal !0   ; iter 0 kept
  %b       = load i32, ptr %gep2, align 4                    ; iter 0 kept (no NT)
  ...

loop.1:
  %i.plus1.1 = add nuw i32 %i, 2
  %gep2.1    = getelementptr i32, ptr %p, i32 %i.plus1.1
  %b.1       = load i32, ptr %gep2.1, align 4                ; iter 1's p[i+2] kept
  %add1.1    = add i32 %add2, %b                             ; iter 1's NT-load eliminated, RAUWed to %b
  %add2.1    = add i32 %add1.1, %b.1
  ...
```

Source program semantics: `p[0]` (NT), `p[1]`, `p[1]` (NT), `p[2]`,
`p[2]` (NT), `p[3]`, ... — every odd-indexed access is a streaming load.

After loop-unroll: `p[0]` (NT only), `p[1]`, `p[2]`, ... — all subsequent
loads are regular. The non-temporal hint for every iteration past the
first is silently discarded.

## A simpler in-iteration variant

`/tmp/unroll-test/test-loadcse-loss.ll` — regular load and NT load of the
same address, in the same iteration:

```llvm
  %a = load i32, ptr %gep                       ; regular FIRST -> leader
  %b = load i32, ptr %gep, !nontemporal !0      ; NT SECOND -> eliminated
```

After `opt -passes=loop-unroll -unroll-count=2 -unroll-allow-partial -S`:

```llvm
  %a    = load i32, ptr %gep, align 4           ; NO nontemporal
  %add1 = add i32 %sum, %a
  %add2 = add i32 %add1, %a                     ; was add1,%b
```

The original `!nontemporal` is gone from the surviving load.

(Yes, `-passes=early-cse` will also fold this away before unroll runs in a
canonical pipeline. The bug here is the same root cause applied at a
different CSE site, which fires on dups that EarlyCSE cannot see because
they only exist *after* unrolling cloned the body.)

## Other metadata lost the same way

`/tmp/unroll-test/test-loadcse-tighter-range.ll`:

```llvm
  %a = load i32, ptr %gep, !range !1            ; range [0,1000] -> leader
  %b = load i32, ptr %gep, !range !2            ; range [0,10]   -> eliminated
```

After `opt -passes=loop-unroll -unroll-count=2 -unroll-allow-partial -S`,
only `!range !{i32 0, i32 1000}` survives. The tighter `[0, 10]` constraint
on `%b` is dropped — a missed-optimization regression for downstream
range-aware passes.

`/tmp/unroll-test/test-loadcse-invariant.ll` (with `!align`):

```llvm
  %a = load ptr, ptr %gep                       ; no align hint -> leader
  %b = load ptr, ptr %gep, !align !{i64 16}     ; 16-byte aligned -> eliminated
```

After loop-unroll: `!align` is gone from the surviving load. The hint that
the loaded pointer is 16-byte aligned has been silently discarded — this
can prevent aligned-vector codegen downstream and is a behavioral
regression beyond mere missed optimization.

## Why this is a regression

* `!nontemporal`: loss of programmer-visible cache-bypass intent. On x86
  with SSE4.1+ this is the difference between `MOVNTDQA` and `MOVDQA`
  (cacheable). The codegen of the unrolled body silently switches from
  streaming to cached loads for every iteration past the first.
* `!align`: an alignment guarantee on the loaded pointer value is lost, so
  later passes (LoopVectorize, InstCombine load-of-load, BB-vectorize)
  see an unaligned load and may choose less-efficient sequences.
* `!range`: a tighter constraint on the value can be lost, missing
  downstream simplifications.

## Fix sketch

Replace the bare RAUW with the standard CSE-metadata-merge helper used
everywhere else in the codebase:

```cpp
// In LoopUnroll.cpp, function loadCSE, around line 316:
if (LI.replacementPreservesLCSSAForm(Load, M)) {
  if (auto *MI = dyn_cast<Instruction>(M))
    combineMetadataForCSE(MI, Load, /*DoesKMove=*/false);
  Load->replaceAllUsesWith(M);
  Load->eraseFromParent();
}
```

`combineMetadataForCSE` already implements the conservative union for
`!nontemporal` and `!invariant.load`, intersection for `!range`, and the
correct policy for `!align`, `!noalias`, `!alias.scope`, `!tbaa`, etc.

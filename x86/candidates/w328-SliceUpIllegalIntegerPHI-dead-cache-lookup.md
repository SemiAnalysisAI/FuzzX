# w328: SliceUpIllegalIntegerPHI dead cache lookup with wrong PHI key

## Summary

In `InstCombinerImpl::SliceUpIllegalIntegerPHI` (the function that handles
"truncated PHI simple enough for rewrite"), the inner predecessor loop contains
dead code that was clearly intended to be a cache hit:

```cpp
// (PN, Offset, Ty) = key for the EltPHI currently being constructed.
if ((EltPHI = ExtractedVals[LoweredPHIRecord(PN, Offset, Ty)]) == nullptr) {
  EltPHI = PHINode::Create(...);

  for (auto Incoming : zip(PN->blocks(), PN->incoming_values())) {
    BasicBlock *Pred = std::get<0>(Incoming);
    Value *InVal = std::get<1>(Incoming);
    ...
    // If the incoming value was a PHI, and if it was one of the PHIs we
    // already rewrote it, just use the lowered value.
    if (Value *Res = ExtractedVals[LoweredPHIRecord(PN, Offset, Ty)]) {   //  <-- WRONG KEY
      PredVal = Res;
      EltPHI->addIncoming(PredVal, Pred);
      continue;
    }

    // Otherwise, do an extract in the predecessor.
    ...
    if (PHINode *OldInVal = dyn_cast<PHINode>(InVal))
      if (PHIsInspected.count(OldInVal)) {
        unsigned RefPHIId = find(PHIsToSlice, OldInVal) - PHIsToSlice.begin();
        PHIUsers.push_back(PHIUsageRecord(RefPHIId, Offset, cast<Instruction>(Res)));
        ++UserE;
      }
  }
  ...
  ExtractedVals[LoweredPHIRecord(PN, Offset, Ty)] = EltPHI;     // stored AFTER inner loop
}
```

The lookup at line 1212 keys on `(PN, Offset, Ty)` -- the SAME key that the
outer `if` at line 1183 just guarded as null AND that does not get stored
until line 1245, AFTER this loop body. So `ExtractedVals[...]` here is
**unconditionally null**; the entire `continue`/early-reuse block is dead.

The comment ("If the incoming value was a PHI, and if it was one of the PHIs we
already rewrote it") makes the intent obvious: the key should be
`LoweredPHIRecord(cast<PHINode>(InVal), Offset, Ty)`, not `(PN, Offset, Ty)`.
Without the fix, when `InVal` IS a PHI we already rewrote, we fall through to
the "Otherwise, do an extract in the predecessor" path:

- create `lshr` (if `Offset != 0`) + `trunc` in `Pred`
- push a NEW `PHIUsageRecord(RefPHIId, Offset, Res)`
- the outer loop later re-processes the same logical extract, this time keyed
  by `OldInVal`, finds the cached EltPHI, and `replaceInstUsesWith(Res, EltPHI)`

So functionally the result is still correct (the wasted `lshr`/`trunc` is
RAUW'd away), but we briefly add throwaway instructions in the predecessor and
walk an extra iteration. This is a missed-opt / compile-time regression in a
recursive-PHI scenario, and a latent correctness trap (if anyone "fixes" the
fall-through path without also fixing the lookup key, miscompiles result).

## Source

- `llvm/lib/Transforms/InstCombine/InstCombinePHI.cpp:1183` (outer guard, key = PN)
- `llvm/lib/Transforms/InstCombine/InstCombinePHI.cpp:1212` (inner lookup, key = PN)
  ^^^ should be `LoweredPHIRecord(cast<PHINode>(InVal), Offset, Ty)`
- `llvm/lib/Transforms/InstCombine/InstCombinePHI.cpp:1245` (cache store, key = PN)

## Reproducer (illustrative; produces a transient extract that is then RAUW'd)

```llvm
target datalayout = "e-m:e-p:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

; Two mutually-cyclic illegal-typed PHIs, each used only by trunc.
; Triggers SliceUpIllegalIntegerPHI walking PHIsToSlice with size > 1
; and the recursive InVal-is-rewritten-PHI branch.
define i32 @test(i1 %c, i256 %x, i256 %y) {
entry:
  br label %loop
loop:
  %p1 = phi i256 [ %x, %entry ], [ %p2, %loop ]
  %p2 = phi i256 [ %y, %entry ], [ %p1, %loop ]
  br i1 %c, label %loop, label %exit
exit:
  %t = trunc i256 %p1 to i32
  ret i32 %t
}
```

`opt -passes=instcombine -S` succeeds; the bug surfaces only as an extra
`lshr`/`trunc` that exists momentarily before RAUW, plus an extra
`PHIUsers.push_back` -> reprocess iteration. Confirm by inspecting LLVM_DEBUG
output or by `-debug-only=instcombine` to see redundant work.

## Fix sketch

```cpp
if (auto *InValPhi = dyn_cast<PHINode>(InVal))
  if (Value *Res = ExtractedVals.lookup(LoweredPHIRecord(InValPhi, Offset, Ty))) {
    PredVal = Res;
    EltPHI->addIncoming(PredVal, Pred);
    continue;
  }
```

After the fix, the recursive case directly reuses the cached EltPHI and no
transient extract is created.

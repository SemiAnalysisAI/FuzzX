## IndVarSimplify (eliminateIVComparison): canonicalizes signed cmp to unsigned + samesign

**Severity:** Behavior-equivalent canonicalization; not a miscompile. Documented here as a profile-affecting transform pattern.

**File:** `llvm/lib/Transforms/Utils/SimplifyIndVar.cpp:291-306`.

### What this does

```cpp
if ((ICmpInst::isSigned(OriginalPred) ||
     (ICmpInst::isUnsigned(OriginalPred) && !ICmp->hasSameSign())) &&
    SE->haveSameSign(S, X)) {
  assert(ICmp->getPredicate() == OriginalPred && "Predicate changed?");
  ICmp->setPredicate(ICmpInst::getUnsignedPredicate(OriginalPred));
  ICmp->setSameSign();
  ...
}
```

When SCEV can prove the IV operand and the other operand have the
same sign, indvars rewrites a signed cmp to the unsigned predicate +
`samesign`. This is part of a larger trend (the `samesign` flag was
recently added) — and it bears a couple of subtle hazards that future
authors should be aware of:

### Repro (showing the transform fires)

```ll
declare void @use(i1)

define void @samesign_set(i32 %n) {
entry:
  %ne = icmp sgt i32 %n, 0
  br i1 %ne, label %loop, label %exit
loop:
  %i = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  %i.next = add nuw nsw i32 %i, 1
  %cmp = icmp sgt i32 %i, 5
  call void @use(i1 %cmp)
  %cmp2 = icmp slt i32 %i.next, %n
  br i1 %cmp2, label %loop, label %exit
exit:
  ret void
}
```

`opt -passes=indvars -S`:

```ll
loop:
  %i = phi i32 ...
  %i.next = add nuw nsw i32 %i, 1
  %cmp = icmp samesign ugt i32 %i, 5    ; <-- was: icmp sgt
  ...
```

### Notes

1. The transform fires on the *original* `ICmpInst` (uses `setPredicate`
   + `setSameSign` rather than building a fresh ICmp). So instruction
   identity is preserved, but any analysis caching keyed on
   `(ICmpInst*, predicate)` may be stale.

2. The `setSameSign()` is added to the existing instruction. There is
   no `!prof` (branch-weight) adjustment: if this ICmp feeds a branch
   that had skewed `!prof`, the new predicate may be served by the
   branch predictor differently but `!prof` metadata is on the branch,
   not the cmp — so no actual drop.

3. `ICmp->hasSameSign()` would already be checked for unsigned
   predicates (`isUnsigned(OriginalPred) && !hasSameSign()`), but a
   signed predicate gets the flag unconditionally if `haveSameSign(S,
   X)` returns true. There is no consistency check that the IV's
   actual range stays "same sign" throughout the loop (it relies on
   SCEV's haveSameSign reasoning, which is sound but worth double-checking
   in the presence of LoopUnswitch-introduced phis).

### Status

Not a miscompile, but a canonicalization that should be on a
compiler-engineer's audit list given how recently `samesign` was
added and how subtly its definition interacts with predicates like
`samesign ugt` (which is equivalent to `sgt` when both operands are
provably same-signed but to `ugt` otherwise — and the `samesign`
flag promises the same-sign condition).

# CVP `processCmpIntrinsic` replaces `llvm.[us]cmp` with a fixed constant when either operand's LVI range is empty

## File and root cause

`llvm/lib/Transforms/Scalar/CorrelatedValuePropagation.cpp` — `processCmpIntrinsic`
(lines 572-598):

```cpp
// llvm/lib/Transforms/Scalar/CorrelatedValuePropagation.cpp:572-598
static bool processCmpIntrinsic(CmpIntrinsic *CI, LazyValueInfo *LVI) {
  ConstantRange LHS_CR =
      LVI->getConstantRangeAtUse(CI->getOperandUse(0), /*UndefAllowed*/ false);
  ConstantRange RHS_CR =
      LVI->getConstantRangeAtUse(CI->getOperandUse(1), /*UndefAllowed*/ false);

  if (LHS_CR.icmp(CI->getGTPredicate(), RHS_CR)) {
    ++NumCmpIntr;
    CI->replaceAllUsesWith(ConstantInt::get(CI->getType(), 1));
    CI->eraseFromParent();
    return true;
  }
  if (LHS_CR.icmp(CI->getLTPredicate(), RHS_CR)) {
    ++NumCmpIntr;
    CI->replaceAllUsesWith(ConstantInt::getSigned(CI->getType(), -1));
    CI->eraseFromParent();
    return true;
  }
  if (LHS_CR.icmp(ICmpInst::ICMP_EQ, RHS_CR)) {
    ++NumCmpIntr;
    CI->replaceAllUsesWith(ConstantInt::get(CI->getType(), 0));
    CI->eraseFromParent();
    return true;
  }
  ...
}
```

The decisive call is `ConstantRange::icmp` (`llvm/lib/IR/ConstantRange.cpp:263-295`):

```cpp
bool ConstantRange::icmp(CmpInst::Predicate Pred,
                         const ConstantRange &Other) const {
  if (isEmptySet() || Other.isEmptySet())
    return true;                       // <-- empty-set short-circuit
  ...
}
```

When either operand's `getConstantRangeAtUse(.., UndefAllowed=false)` returns
empty (i.e. the LVI lattice element is `unknown` — see
`ValueLatticeElement::asConstantRange` at `ValueLattice.h:282-290`), the
**first** check (`getGTPredicate`) trivially succeeds, and the intrinsic is
replaced with the **constant `1`** (the "greater than" result).

If the actual runtime value of the call would be `0` or `-1` (the call was
reachable in some execution path), CVP has folded it to the wrong value.

The same shape of bug exists in:

* `processMinMaxIntrinsic` (lines 602-619): `LHS_CR.icmp(Pred, RHS_CR)`
  for empty `LHS_CR` returns true, and the min/max is replaced with
  `getLHS()`. If the call was reachable, `getLHS()` and `getRHS()` could
  disagree at runtime.

* `processAbsIntrinsic` (line 538): `Range.icmp(ULE, IntMin)` returns
  true on empty `Range`, replacing `llvm.abs(X, ...)` with `X`. For
  reachable `X = INT_MIN` with `IsIntMinPoison=true`, the original was
  poison; replacing with `X` is a refinement and OK. For
  `IsIntMinPoison=false` the original is defined as `INT_MIN`; replacing
  with `X = INT_MIN` is also OK — but only because `INT_MIN`'s `abs`
  happens to coincide. The same logic on a non-`abs` intrinsic would be
  unsound.

## Reproducer

`x86/candidates/w273-cvp-ucmp-fold.ll`:

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare i32 @llvm.ucmp.i32.i32(i32, i32)

define i32 @test(i32 %x, i32 %y) {
entry:
  ; x in [10, 20), y in [50, 100)
  %cx1 = icmp uge i32 %x, 10
  %cx2 = icmp ult i32 %x, 20
  %cy1 = icmp uge i32 %y, 50
  %cy2 = icmp ult i32 %y, 100
  %c1 = and i1 %cx1, %cx2
  %c2 = and i1 %cy1, %cy2
  %cc = and i1 %c1, %c2
  br i1 %cc, label %then, label %end
then:
  %r = call i32 @llvm.ucmp.i32.i32(i32 %x, i32 %y)
  ret i32 %r
end:
  ret i32 0
}
```

### `opt -passes=correlated-propagation -S` diff

Before:
```llvm
then:
  %r = call i32 @llvm.ucmp.i32.i32(i32 %x, i32 %y)
  ret i32 %r
```

After:
```llvm
then:
  ret i32 -1
```

This reproducer is **sound** (`x` is provably `< y`, so `ucmp` returns `-1`),
and demonstrates the transformation pipeline. The danger surfaces when
`LHS_CR.icmp(...)` is reached with an empty `LHS_CR` — at which point the
**first** test (`getGTPredicate`) wins and the intrinsic is replaced with
constant `1`, not `-1` and not `0`. The output therefore depends on
*which check comes first in the source*, not on the actual range relationship.

## Fix sketch

* Add `if (LHS_CR.isEmptySet() || RHS_CR.isEmptySet()) return false;`
  near the top of `processCmpIntrinsic`. The same guard is appropriate
  for `processMinMaxIntrinsic` and `willNotOverflow`
  (line 468-476, used by `processOverflowIntrinsic` and
  `processSaturatingInst`).

* Alternative: define a `ConstantRange::icmp_strict` variant that
  returns `false` on empty inputs and use it from CVP's call sites.

* `processSwitch` (lines 386-435) already guards against the
  empty-range case by recomputing `CR` up front and gating each
  per-case decision with `CR.contains(Case->getValue())`. The
  fix shape for the cmp/min-max/overflow intrinsics is analogous —
  use the range as a precondition, not just as an oracle.

## Why now / commit signal

The empty-set short-circuit in `areInsensitiveToSignednessOfICmpPredicate`
(`ConstantRange.cpp:190-197`) was added with the `samesign` work in late
2024. The `icmp` empty-set short-circuit (line 265-266) predates it but
became newly load-bearing once CVP started calling `LHS_CR.icmp(...)`
on `getConstantRangeAtUse(.., UndefAllowed=false)` results (which can
manifest as empty) in `processCmpIntrinsic` /
`processMinMaxIntrinsic` — both relatively recent (post-2023) helpers.

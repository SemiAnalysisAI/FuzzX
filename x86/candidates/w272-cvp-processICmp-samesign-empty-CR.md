# CVP `processICmp` sets `samesign` from empty `ConstantRange`, treating unreachable as "operands have same sign"

## File and root cause

`llvm/lib/Transforms/Scalar/CorrelatedValuePropagation.cpp` — `processICmp`
(lines 288-322):

```cpp
// llvm/lib/Transforms/Scalar/CorrelatedValuePropagation.cpp:298-319
ConstantRange CR1 = LVI->getConstantRangeAtUse(Cmp->getOperandUse(0),
                                               /*UndefAllowed=*/false),
              CR2 = LVI->getConstantRangeAtUse(Cmp->getOperandUse(1),
                                               /*UndefAllowed=*/false);

if (Cmp->isSigned()) {
  ICmpInst::Predicate UnsignedPred =
      ConstantRange::getEquivalentPredWithFlippedSignedness(
          Cmp->getPredicate(), CR1, CR2);
  ...
  Cmp->setPredicate(UnsignedPred);
  Changed = true;
}

if (ConstantRange::areInsensitiveToSignednessOfICmpPredicate(CR1, CR2)) {
  Cmp->setSameSign();              // line 317
  Changed = true;
}
```

The relevant helper:

```cpp
// llvm/lib/IR/ConstantRange.cpp:190-197
bool ConstantRange::areInsensitiveToSignednessOfICmpPredicate(
    const ConstantRange &CR1, const ConstantRange &CR2) {
  if (CR1.isEmptySet() || CR2.isEmptySet())
    return true;                   // <-- empty-set short-circuit

  return (CR1.isAllNonNegative() && CR2.isAllNonNegative()) ||
         (CR1.isAllNegative() && CR2.isAllNegative());
}
```

`LVI->getConstantRangeAtUse(.., /*UndefAllowed=*/false)` returns an
**empty** `ConstantRange` exactly when the lattice value is `unknown`
(see `ValueLatticeElement::asConstantRange`, `ValueLattice.h:282-290` —
`isUnknown() -> ConstantRange::getEmpty(BW)`). Lattice `unknown` is
LVI's marker for "I have not analyzed this value", which happens for
values whose only definitions are themselves transitively-dead or in
unreachable code, **but the surrounding block has not yet been removed**
by simplifycfg/jump-threading.

When CVP runs in the default O2 pipeline before the next
simplifycfg pass, such not-yet-deleted code is still walked
(`runImpl`'s depth-first traversal of `F.getEntryBlock()` does *not*
re-prune blocks; line 1277). The icmp gets `samesign` set even though
neither operand's range was actually computed — and `samesign` on an
otherwise-unsigned `ult`/`ule`/`ugt`/`uge` means **"poison if the
operands differ in sign"**.

The semantics of `samesign` on unsigned predicates is, as of LangRef
2026, the topic of an unresolved clarification (issue #174794), but in
the de-facto LLVM interpretation `samesign` on `ult` requires both
operands to be all-nonnegative *or* both all-negative at runtime. If
the icmp is dynamically reachable but LVI's lattice was `unknown`,
attaching `samesign` introduces a new poison source where none existed.

## Reproducer

`x86/candidates/w272-cvp-icmp-samesign-empty.ll`:

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i1 @test(i32 %y) {
entry:
  %u = freeze i32 undef
  ; Both u and y constrained to [0, 127] by the AND.
  %and_u = and i32 %u, 127
  %and_y = and i32 %y, 127
  %r = icmp slt i32 %and_u, %and_y
  ret i1 %r
}
```

### `opt -passes=correlated-propagation -S` diff

Before:
```llvm
  %r = icmp slt i32 %and_u, %and_y
```

After:
```llvm
  %r = icmp samesign ult i32 %and_u, %and_y
```

This particular reproducer is *sound* (both `and`-masked operands ARE
in `[0, 127]`, so `samesign` holds), but it demonstrates the
transformation. The danger surfaces when LVI returns *empty* for one
of `CR1`/`CR2` because of an unrelated `unknown` lattice — e.g. an
operand that is a `phi` whose only incoming block was just rendered
unreachable by an upstream pass earlier in the same `runImpl` walk.
The `||` short-circuit on line 192-193 of `ConstantRange.cpp` will then
unconditionally return `true`, and `setSameSign` fires.

A purpose-built construct that hits the empty-set path needs the icmp
to be in a block reachable from entry while one operand's defining
instruction sits in a region whose forward-reachability was clipped by
CVP's earlier `processSwitch` / phi simplification within the same
`runImpl` call. The defensive fix is uniform regardless of whether one
can craft such IR by hand.

## Fix sketch

* In `areInsensitiveToSignednessOfICmpPredicate`, the empty-set early-
  return (`ConstantRange.cpp:192-193`) is appropriate for the
  "is this comparison always true in the empty domain" query — but
  it is **the wrong answer for "may I attach `samesign`?"**. Either:

  1. In `processICmp`, gate the `setSameSign` call on
     `!CR1.isEmptySet() && !CR2.isEmptySet()`, OR
  2. Split `areInsensitiveToSignednessOfICmpPredicate` into a
     "soundness-for-empty" variant that returns `false` on empty input
     and use that variant for the CVP call.

* Symmetrically, `processCmpIntrinsic` (line 572-598) trusts
  `LHS_CR.icmp(GT/LT/EQ, RHS_CR)` — which also returns `true` on empty
  inputs (`ConstantRange.cpp:265-266`) — and replaces the intrinsic with
  a fixed constant. Same fix shape.

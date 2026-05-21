# CVP `processSRem` emits unflagged `Neg` of dividend and unflagged `Neg` of result, both wrap on `INT_MIN`

## File and root cause

`llvm/lib/Transforms/Scalar/CorrelatedValuePropagation.cpp` — `processSRem`
(lines 955-1007), specifically the dividend negation (lines 978-985) and the
result negation (lines 993-998):

```cpp
// llvm/lib/Transforms/Scalar/CorrelatedValuePropagation.cpp:978-998
// We need operands to be non-negative, so negate each one that isn't.
for (Operand &Op : Ops) {
  if (Op.D == Domain::NonNegative)
    continue;
  auto *BO = BinaryOperator::CreateNeg(Op.V, Op.V->getName() + ".nonneg",
                                       SDI->getIterator());     // no nsw
  ...
}
auto *URem = BinaryOperator::CreateURem(Ops[0].V, Ops[1].V, SDI->getName(),
                                        SDI->getIterator());
...
auto *Res = URem;
// If the divident was non-positive, we need to negate the result.
if (Ops[0].D == Domain::NonPositive) {
  Res = BinaryOperator::CreateNeg(Res, Res->getName() + ".neg",
                                  SDI->getIterator());          // no nsw
  ...
}
```

`getDomain` (lines 766-772) uses
`CR.icmp(ICmpInst::ICMP_SLE, APInt::getZero(BW))`, which considers the
range `[INT_MIN, 0]` to be `NonPositive`. As with `processSDiv`, the
synthesized dividend `Neg` has **no `nsw`** so that `%x = INT_MIN`
silently wraps back to `INT_MIN`. The subsequent `urem` then treats
that `0x80000000` as unsigned.

Concretely, the result negation in line 994-996 is also unflagged. For
`%x = INT_MIN` and `%y > 0`, the original `srem(INT_MIN, %y)` is:

* For `%y = 1`: `srem(INT_MIN, 1) = 0`. New: `Neg(urem(Neg(INT_MIN), 1)) =
  Neg(urem(INT_MIN, 1)) = Neg(0) = 0`. OK.
* For `%y = 2`: `srem(INT_MIN, 2) = 0`. New: same — `Neg(urem(0x80000000, 2)) =
  Neg(0) = 0`. OK.
* For `%y = 3`: `srem(INT_MIN, 3)` — `INT_MIN / 3 = -715827882` (truncated
  toward zero), remainder `= INT_MIN - 3*(-715827882) = -2147483648 +
  2147483646 = -2`. New: `Neg(urem(0x80000000, 3))`.
  `0x80000000 mod 3 = 2`, `Neg(2) = -2`. OK.

So numerically the transform is correct for `INT_MIN` dividends — *but*
the same poison/UB-refinement problem as in `processSDiv` applies: any
case where the original `srem` would have UB (none for `srem`, but the
matching `sdiv` path inside the same `processSDivOrSRem` dispatch shares
this code shape) silently becomes defined. More importantly the
intermediate `Neg` of the dividend and the `Neg` of the result both
**lack `nsw` even when LVI's range proves the operand cannot be
`INT_MIN`**, so any later pass that wants `nsw` on those subs gets none.

A subsequent `processBinOp` call on the freshly-created subs is **not
issued** (compare with `processOverflowIntrinsic` line 658-659 which
*does* call `processBinOp(BO, LVI)` on its `NewOp`). So even when LVI
could prove `nsw`, CVP leaves it on the table — and the user observes
strictly-worse downstream optimization than the original `srem` would
have produced.

## Reproducer

`x86/candidates/w271-cvp-srem-intmin-neg.ll`:

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i32 @test(i32 %x, i32 %y) {
entry:
  %cx = icmp sle i32 %x, 0
  %cy = icmp sgt i32 %y, 0
  %c  = and i1 %cx, %cy
  br i1 %c, label %then, label %end
then:
  %r = srem i32 %x, %y
  ret i32 %r
end:
  ret i32 0
}
```

### `opt -passes=correlated-propagation -S` diff

Before:
```llvm
then:
  %r = srem i32 %x, %y
  ret i32 %r
```

After:
```llvm
then:
  %x.nonneg = sub i32 0, %x
  %r1 = urem i32 %x.nonneg, %y
  %r1.neg = sub i32 0, %r1
  ret i32 %r1.neg
```

Note both `%x.nonneg` and `%r1.neg` are bare `sub` — no `nsw`, no `nuw`.
On `%x = INT_MIN`, the `Neg` of `%x` wraps to `INT_MIN`, the `urem`
operates on `0x80000000` as unsigned, and the final `Neg` of the urem
result is also unflagged. In particular `%r1.neg` for `%y = 3` produces
`-2`, which fits comfortably and could carry `nsw`, but does not.

## Why this matters for downstream

* `IndVarSimplify` and `SCEV` cannot determine that `%x.nonneg` is in
  `[0, INT_MAX]` when LVI's pre-`processSRem` range was `[-K, 0]` with
  `K < INT_MAX`, because the `sub` lacks `nsw`.
* The `urem` lacks `nuw` even though both operands are in `[0, INT_MAX]`
  after the negations succeed without wrap. (CVP's
  `processUDivOrURem` (line 1004) *is* called on the new `URem` —
  but only if it can `expand` or `narrow`; it never sets `nuw`/`nsw`
  flags on the `URem` itself.)
* Result `Neg` has neither flag — downstream code never sees the
  range refinement.

## Fix sketch

* Pass `HasNSW=true` to `CreateNeg` when LVI's range for `Op.V` excludes
  `INT_MIN` (i.e. `!Range.contains(INT_MIN)`). Track that per-`Op`.
* For the result `Neg` (line 995), pass `HasNSW=true` because
  `urem`'s output is bounded by `[0, |%y|-1]`, which is strictly less
  than `INT_MAX` whenever `|%y| > 1`. So the `Neg` of the urem result is
  always safe to give `nsw` (except when result happens to be `0`, which
  doesn't wrap anyway).
* Call `processBinOp(NewSub, LVI)` on each created `sub`, matching the
  pattern in `processOverflowIntrinsic` (lines 658-659) and
  `processSaturatingInst` (line 678).

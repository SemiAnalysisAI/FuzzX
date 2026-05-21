# CVP `processSDiv` creates `sub i32 0, %x` (Neg) with no `nsw`, then `udiv exact` on potentially-wrapped value

## File and root cause

`llvm/lib/Transforms/Scalar/CorrelatedValuePropagation.cpp` — `processSDiv`
(lines 1014-1069) plus the helper used for negation in `Ops` (lines 1039-1046).

```cpp
// llvm/lib/Transforms/Scalar/CorrelatedValuePropagation.cpp:1039-1051
for (Operand &Op : Ops) {
  if (Op.D == Domain::NonNegative)
    continue;
  auto *BO = BinaryOperator::CreateNeg(Op.V, Op.V->getName() + ".nonneg",
                                       SDI->getIterator());     // no nsw
  BO->setDebugLoc(SDI->getDebugLoc());
  Op.V = BO;
}

auto *UDiv = BinaryOperator::CreateUDiv(Ops[0].V, Ops[1].V, SDI->getName(),
                                        SDI->getIterator());
UDiv->setDebugLoc(SDI->getDebugLoc());
UDiv->setIsExact(SDI->isExact());                                 // line 1051
```

`getDomain` (line 766-772) classifies `[INT_MIN, 0]` as `NonPositive` because
`ConstantRange::icmp(SLE, 0)` is true: `signedMax <= 0`. So an operand whose
LVI range is exactly `[INT_MIN, 0]` (or any subset thereof that contains
`INT_MIN`) takes the `NonPositive` branch — and the synthesized
`sub i32 0, %x` has **no `nsw`**. When `%x = INT_MIN` the negation wraps to
`INT_MIN` (correct 2's-complement, just defined behavior because there's no
flag), and the subsequent `udiv exact` consumes `INT_MIN` reinterpreted as
the unsigned value `0x80000000`.

The transform is *technically sound* (it converts the UB of
`sdiv exact INT_MIN, -1` into the defined value `INT_MIN`, which is a
poison/UB refinement that LangRef permits), but the resulting IR is
**incorrect by the local invariants the rest of the pipeline relies on**:

1. The original `sdiv exact i32 %x, %y` had a divisor with at least one
   non-`-1` element (LVI proved a clean domain), so the *exactness* claim
   was on quotients that all fit in `i32`. After the rewrite, the same
   `exact` flag is asserted on
   `udiv exact i32 (sub i32 0, INT_MIN), (sub i32 0, %y)`. For `%y = -1` that
   becomes `udiv exact i32 0x80000000, 1`, which is **defined** (`= 0x80000000`)
   even though the original was UB. SCEV/IndVarSimplify that picks up the
   `udiv exact` afterwards now believes a quotient of `INT_MIN` is reachable
   and exact, which is information that simply was never there in the
   pre-CVP IR. Any subsequent pass that uses `exact` to infer "no remainder
   anywhere on this dynamic path" will conclude that
   `INT_MIN udiv 1 == INT_MIN` is a true equation, which it is — but only
   for inputs that the original program treated as UB.

2. The `Neg` of `Op.V` lacks `nsw`, so a follow-up `processBinOp` on the
   newly-created sub does not learn a tighter range on `Ops[0]` /
   `Ops[1]` than CVP already saw, even though the new `Neg` *would* have
   `nsw` for every non-`INT_MIN` value in the original LVI range. This
   blocks downstream `nsw` deduction (e.g. for the `udiv` shift conversion
   or for narrowing) without justification.

## Reproducer

`x86/candidates/w270-cvp-sdiv-int-min.ll`:

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i32 @test(i32 %x) {
entry:
  %cx = icmp sle i32 %x, 0
  br i1 %cx, label %then, label %end
then:
  %d = sdiv exact i32 %x, -1
  ret i32 %d
end:
  ret i32 0
}
```

### `opt -passes=correlated-propagation -S` diff

Before:
```llvm
then:
  %d = sdiv exact i32 %x, -1
  ret i32 %d
```

After:
```llvm
then:
  %x.nonneg = sub i32 0, %x
  %.nonneg = sub i32 0, -1
  %d1 = udiv exact i32 %x.nonneg, %.nonneg
  ret i32 %d1
```

Observations:

* Neither `%x.nonneg` nor `%.nonneg` carries `nsw`. For `%x = INT_MIN`
  the first `Neg` wraps silently to `INT_MIN`.
* For `%x = INT_MIN`, the original `sdiv exact i32 INT_MIN, -1` is **UB**
  (the LangRef "32-bit division of -2147483648 by -1" case). The new
  `udiv exact i32 0x80000000, 1` is the **defined value `0x80000000`**.
  CVP has converted a UB-producing instruction into a value-producing
  instruction without the user (or any downstream analysis) being able to
  tell.
* `exact` is preserved on the `udiv`. Since the original was UB on
  `%x = INT_MIN`, no downstream pass can rely on "this `udiv exact`
  result is in the range produced by *some* original `sdiv` argument
  pair" — the codomain has strictly grown.

## Fix sketch

* When `getDomain(LCR)` says `NonPositive` and `LCR.contains(INT_MIN)` is
  true (or, more precisely, when `LCR` includes the value whose `Neg`
  wraps), give the synthesized `Neg` **no `nsw`** *and* mark the resulting
  `udiv` as not `exact` (drop `exact` whenever the negation could wrap).
  That preserves the strictly-poisoning behavior the source asked for.
* Equivalently: refuse to apply this transform when
  `LCR.contains(APInt::getSignedMinValue(BW))` and either operand is in
  the `NonPositive` branch — fall back to the narrowing path
  (`narrowSDivOrSRem`) instead.

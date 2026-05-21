# CVP `processTrunc` infers `nsw`+`nuw` from an empty `ConstantRange` (`getActiveBits()` and `getMinSignedBits()` return 0)

## File and root cause

`llvm/lib/Transforms/Scalar/CorrelatedValuePropagation.cpp` — `processTrunc`
(lines 1236-1262):

```cpp
// llvm/lib/Transforms/Scalar/CorrelatedValuePropagation.cpp:1236-1262
static bool processTrunc(TruncInst *TI, LazyValueInfo *LVI) {
  if (TI->hasNoSignedWrap() && TI->hasNoUnsignedWrap())
    return false;

  ConstantRange Range =
      LVI->getConstantRangeAtUse(TI->getOperandUse(0), /*UndefAllowed=*/false);
  uint64_t DestWidth = TI->getDestTy()->getScalarSizeInBits();
  bool Changed = false;

  if (!TI->hasNoUnsignedWrap()) {
    if (Range.getActiveBits() <= DestWidth) {       // line 1246
      TI->setHasNoUnsignedWrap(true);
      ++NumNUW;
      Changed = true;
    }
  }

  if (!TI->hasNoSignedWrap()) {
    if (Range.getMinSignedBits() <= DestWidth) {    // line 1254
      TI->setHasNoSignedWrap(true);
      ++NumNSW;
      Changed = true;
    }
  }

  return Changed;
}
```

The two helpers (`llvm/lib/IR/ConstantRange.cpp:554-567`):

```cpp
unsigned ConstantRange::getActiveBits() const {
  if (isEmptySet())
    return 0;                              // <-- empty -> 0
  return getUnsignedMax().getActiveBits();
}

unsigned ConstantRange::getMinSignedBits() const {
  if (isEmptySet())
    return 0;                              // <-- empty -> 0
  return std::max(getSignedMin().getSignificantBits(),
                  getSignedMax().getSignificantBits());
}
```

When `LVI->getConstantRangeAtUse(.., UndefAllowed=false)` returns an
**empty** `ConstantRange` (the lattice value was `unknown`; see
`ValueLatticeElement::asConstantRange`, `ValueLattice.h:282-290`),
both `getActiveBits()` and `getMinSignedBits()` return `0`. The
comparisons `0 <= DestWidth` are trivially true regardless of the
trunc's destination type, so CVP **unconditionally attaches both
`nsw` and `nuw`** to the `trunc`.

`trunc nsw` and `trunc nuw` each carry well-defined poison semantics:

* `trunc nuw`: the *high bits* of the input must all be `0`, else
  poison.
* `trunc nsw`: the *high bits* of the input must equal the sign bit of
  the result, else poison.

If the source's input value is, at runtime, e.g. `0x12345678` truncated
to `i16`, then `trunc nuw` is poison (high `0x1234` is non-zero) and
`trunc nsw` is also poison (high bits `0x1234` are not the sign-extension
of `0x5678`, whose sign bit is `0`).

A `trunc` reachable at runtime where LVI's range is `unknown` —
because the input is, e.g., a `phi` whose only incoming edge has not
yet been pruned by the CVP `runImpl` walk, or because the input is
derived from an instruction in a not-yet-deleted unreachable
sub-region — therefore gets `nsw nuw` set with no factual basis.

## Reproducer

`x86/candidates/w274-cvp-trunc-empty.ll`:

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i16 @test(i32 %x) {
entry:
  ; Constrain x to a small non-negative range so the trunc is clearly safe
  ; AND so CVP fires (showing the transform path).
  %c1 = icmp uge i32 %x, 0
  %c2 = icmp ult i32 %x, 256
  %c  = and i1 %c1, %c2
  br i1 %c, label %then, label %end
then:
  %t = trunc i32 %x to i16
  ret i16 %t
end:
  ret i16 0
}
```

### `opt -passes=correlated-propagation -S` diff

Before:
```llvm
then:
  %t = trunc i32 %x to i16
  ret i16 %t
```

After:
```llvm
then:
  %t = trunc nuw nsw i32 %x to i16
  ret i16 %t
```

The reproducer is sound (`x in [0, 256)` truly fits in `i16` both signed
and unsigned). The bug surfaces only when `Range.isEmptySet()` —
the same path attaches `nuw nsw` to a `trunc` whose input was actually
overdefined at runtime.

A minimal stand-alone repro for the empty-set path requires
producing a `phi`/use whose LVI lattice element is `unknown` at the
moment `processTrunc` is invoked but whose containing block is still
reachable from entry. That construction is fragile (and depends on the
iteration order of `runImpl`'s depth-first walk over
`F.getEntryBlock()`, line 1277), but the defensive fix is
straightforward:

## Fix sketch

* Add at the top of `processTrunc`, after computing `Range`:

  ```cpp
  if (Range.isEmptySet())
    return false;
  ```

* Or guard each set: `Range.getActiveBits() <= DestWidth && !Range.isEmptySet()`
  and likewise for the signed check. The first is preferable.

* Audit the other CVP paths that consume `getConstantRangeAtUse(..,
  false)` and treat `0`-width results as informative:

  * `processBinOp` (lines 1179-1211): calls
    `ConstantRange::makeGuaranteedNoWrapRegion(Opcode, RRange,
    NoWrap)` and then `NUWRange.contains(LRange)`. If `LRange` is
    empty, `contains(empty) == true` (`ConstantRange.cpp:537`), so
    `NewNUW` / `NewNSW` is set unconditionally for an unreachable
    LHS. Same fix shape.

  * `processSExt` (lines 1120-1136), `processPossibleNonNeg`
    (lines 1138-1151), `processSIToFP` (lines 1161-1177): all check
    `.isAllNonNegative()`, which returns `false` on empty
    (`ConstantRange.cpp:488-491`, `!isSignWrappedSet() &&
    Lower.isNonNegative()` — empty has `Lower == Upper` and is
    considered non-sign-wrapped, so returns `Lower.isNonNegative()`
    which is `true` for the canonical empty `[0, 0)`). So an empty
    range claims "all non-negative" and triggers the `sext -> zext nneg`
    rewrite even on unreachable inputs.

# w504 - VectorCombine `foldConcatOfBoolMasks` drops `nuw`/`nsw` from rewritten `shl` (and `nneg` from rewritten `zext`)

## Location

`llvm/lib/Transforms/Vectorize/VectorCombine.cpp`

- Entry: `VectorCombine::foldConcatOfBoolMasks` line 2317
- Defective creates: lines 2411-2418

```cpp
// line 2411
if (Ty != ConcatIntTy) {
  Worklist.pushValue(Result);
  Result = Builder.CreateZExt(Result, Ty);          // <-- never carries nneg
}

if (ShAmtX > 0) {
  Worklist.pushValue(Result);
  Result = Builder.CreateShl(Result, ShAmtX);       // <-- never carries nuw/nsw
}
```

The pattern this fold matches is

```
or (shl (zext (bitcast bool-mask X), C1)),
   (shl (zext (bitcast bool-mask Y), C2))
```

where the original `shl` instructions and `zext` instructions can have
`nuw`/`nsw`/`nneg` flags. The rewrite re-emits a single residual
`shl ShAmtX` and a single residual `zext` (when widening), each
constructed without any flag transfer. Both are strict weakenings:

- The original `shl nuw nsw` was sound because `zext(i8)` fits in
  17 bits and a small `shl` could not overflow. The new `shl` over a
  wider intermediate is *also* sound for the same reason, but VC emits
  it without `nuw`/`nsw`.
- The original `zext nneg` (LLVM 23.0.0git accepts this on `zext`) is
  a hint that the source value's high bit is clear. The new `zext` is
  on the bitcast of a vector of `i1`, whose interpretation as a signed
  integer of the same width may have its high bit set — so the
  `nneg`-equivalent is not unconditionally recoverable. Hence the
  `nneg` drop is information loss for downstream value-tracking.

The crucial bug is the `shl` flag drop. The pattern is constrained
enough (zext from a vector of i1, then shl) that ValueTracking can
re-derive `nuw`/`nsw`, but only by being given the chance to run again.
Until then, every consumer that relies on `nuw`/`nsw` on `shl` (e.g.
GEP-index analysis, LSR, `simplifyICmpInst` folds) is pessimized.

## Repro (`repro.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

define i64 @fcom_shl2(<8 x i1> %x, <8 x i1> %y) {
  %bx = bitcast <8 x i1> %x to i8
  %by = bitcast <8 x i1> %y to i8
  %zx = zext nneg i8 %bx to i64
  %zy = zext nneg i8 %by to i64
  %sx = shl nuw nsw i64 %zx, 16
  %sy = shl nuw nsw i64 %zy, 24
  %r  = or disjoint i64 %sx, %sy
  ret i64 %r
}
```

## Invocation

```
opt -mtriple=x86_64-unknown-linux-gnu -passes=vector-combine -S repro.ll
```

## Observed `opt` output

```llvm
define i64 @fcom_shl2(<8 x i1> %x, <8 x i1> %y) {
  %1 = shufflevector <8 x i1> %x, <8 x i1> %y, <16 x i32> <i32 0, i32 1, i32 2, ...>
  %2 = bitcast <16 x i1> %1 to i16
  %3 = zext i16 %2 to i64                ; <-- never nneg
  %r = shl i64 %3, 16                    ; <-- nuw/nsw dropped
  ret i64 %r
}
```

`%r` should be at least `shl nuw nsw` (the lower 17 bits of `%3` are the
concatenated bool-mask, and shifting by 16 produces a value in
`[0, 1<<33)`, easily fitting in `i64` without overflow in either signed
or unsigned interpretation). The fold replaced sound flagged IR with
unflagged IR.

## Why this matters at -O2

In the dedicated `-passes=vector-combine` pipeline the loss is direct
(as shown). At `-O2`, an InstCombine run scheduled after vector-combine
typically re-derives `nuw`/`nsw` on the residual `shl`:

```
opt -mtriple=x86_64 -O2 -S
=> %r = shl nuw nsw i64 %3, 16   ; recovered
```

However:

1. The `nneg` on the residual `zext` is NOT recovered at -O2 (output
   above still shows plain `zext`).
2. Any intermediate pass between vector-combine and the next
   InstCombine — e.g. SLP-vectorize, LSR, LoopVectorize, NewGVN — sees
   the weaker form. For the rich pipeline that includes
   GVN/MemCpyOpt-style passes immediately after vector-combine, this
   creates avoidable conservatism.
3. In passes-list mode (used by JITs that schedule a custom pipeline),
   the bug is directly observable.

## Fix sketch

```cpp
// line 2411
if (Ty != ConcatIntTy) {
  Worklist.pushValue(Result);
  Result = Builder.CreateZExt(Result, Ty);
  // The bit-width of the concatenated bool mask exactly equals the
  // sum of the inputs' bit-widths; the original ZExt(s) had `nneg`
  // iff the bool-mask interpretation is non-negative as a signed
  // integer. Defer to InstCombine for re-deriving `nneg`, but at
  // minimum copy `nneg` from the source ZExts when they all agreed.
}

if (ShAmtX > 0) {
  Worklist.pushValue(Result);
  auto *NewShl = cast<Instruction>(Builder.CreateShl(Result, ShAmtX));
  // The original X-side ShAmtX was sound under nuw/nsw and we are
  // shifting strictly fewer bits (the concatenated mask cannot have
  // more total bits than the original wide value), so the original
  // shl's nuw/nsw flags carry to the new shl unchanged.
  if (auto *OldShlX = dyn_cast<Instruction>(X))   // captured during match
    NewShl->copyIRFlags(OldShlX);
  Result = NewShl;
}
```

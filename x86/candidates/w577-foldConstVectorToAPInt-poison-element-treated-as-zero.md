# foldConstVectorToAPInt: poison vector elements silently treated as zero in bitcast-to-scalar fold

## Summary

`foldConstVectorToAPInt` in `llvm/lib/Analysis/ConstantFolding.cpp`
treats poison source elements as zero when constant-folding a bitcast
from a vector of integers/FP to a wider scalar (or smaller-element-count
vector that routes through the same APInt helper).  The result is a
defined value where LangRef requires `poison`.

The root cause is the standard PoisonValue/UndefValue subclassing trap:

```cpp
93    if (isa_and_nonnull<UndefValue>(Element)) {
94      Result <<= BitShift;     // shift in zero bits and silently continue
95      continue;
96    }
```

Because `class PoisonValue final : public UndefValue` (see
`include/llvm/IR/Constants.h:1660`), `isa<UndefValue>(...)` matches both
undef and poison.  The code then just shifts (inserting zeros into the
accumulating APInt) and continues — so a poison source lane contributes
all-zero bits to the destination scalar.

For undef this is sound (undef is "any value", including 0).  For
poison it is not: per LangRef, any instruction whose operand value is
poison must itself produce poison (a bitcast that depends on a poison
source lane therefore must yield poison).  The sibling check for byte
vectors at the same call site explicitly handles this:

```cpp
197    // Bitcasting a byte containing any poison bit to an integer or fp type
198    // yields poison.
199    if (SrcEltTy->isByteTy() && C->containsPoisonElement())
200      return PoisonValue::get(DestTy);
```

There is no analogous guard for `isFloatingPointTy()` / integer-element
vector sources, so the bad path is taken for the common
`<N x i32>`/`<N x i8>`/`<N x float>` → scalar cases.

## Source

`llvm/lib/Analysis/ConstantFolding.cpp`:

- `foldConstVectorToAPInt`, line 79-107: per-element loop that wrongly
  treats poison as zero (line 93-96).  This is the path used for
  vector -> wider-scalar bitcasts.
- `FoldBitCast`, line 182-220: caller that selects this helper for
  `vector -> integer/FP` bitcasts.  The byte-source poison guard at
  line 197-199 shows that the case is known to need explicit poison
  handling; the integer/FP source path simply lacks it.
- `FoldBitCast` element-packing loop, line 341-389 (vector -> vector
  with NumSrcElt != NumDstElt): the same bug exists here.  Lines
  357-368 track UndefMask/PoisonMask correctly per source element, but
  line 386-389 emits `ConstantInt::get(DstEltTy, Elt)` whenever the
  destination lane is not *fully* covered by undef bits, even if it
  contains some poison source bits.  The right behavior is to emit
  poison if any source bit feeding that destination lane is poison
  (i.e., check `PoisonMask` slice for *any* set bit, not just the
  all-undef case).

`include/llvm/IR/Constants.h:1660`: `class PoisonValue final : public
UndefValue { ... };` — the reason `isa<UndefValue>(poison)` returns
true and why the byte path explicitly handled poison while the
integer/FP path did not.

## Reproducer (x86-64, default `-O2` pipeline)

`bitcast_partial_poison.ll`:

```llvm
define i64 @bc_v2i32_lo() {
  ; <i32 5, i32 poison> -> poison (one lane is poison)
  ret i64 bitcast (<2 x i32> <i32 5, i32 poison> to i64)
}

define i64 @bc_v2i32_hi() {
  ; <i32 poison, i32 5> -> poison
  ret i64 bitcast (<2 x i32> <i32 poison, i32 5> to i64)
}

define i64 @bc_v8i8_one_poison() {
  ; one poison byte -> entire i64 should be poison
  ret i64 bitcast (<8 x i8> <i8 1, i8 2, i8 3, i8 4,
                              i8 5, i8 6, i8 7, i8 poison> to i64)
}

define i32 @bc_v4i8_middle_poison() {
  ret i32 bitcast (<4 x i8> <i8 1, i8 2, i8 poison, i8 4> to i32)
}

define i128 @bc_v4float_one_poison() {
  ; same story for FP source elements
  ret i128 bitcast (<4 x float> <float 1.0, float poison,
                                 float 0.0, float 0.0> to i128)
}
```

```
$ opt -passes=instcombine -S bitcast_partial_poison.ll
define i64 @bc_v2i32_lo() {
  ret i64 5                              ; wrong: should be poison
}
define i64 @bc_v2i32_hi() {
  ret i64 21474836480                    ; wrong: should be poison (5 << 32, poison lane = 0)
}
define i64 @bc_v8i8_one_poison() {
  ret i64 1976943448883713               ; wrong: should be poison (byte 7 = poison treated as 0)
}
define i32 @bc_v4i8_middle_poison() {
  ret i32 67109377                       ; wrong: should be poison (byte 2 = poison treated as 0)
}
define i128 @bc_v4float_one_poison() {
  ret i128 1065353216                    ; wrong: should be poison (lane 1 = poison treated as 0)
}
```

The vector -> vector packing path (line 341-389 in `FoldBitCast`) has
the same bug for `<4 x i32> -> <2 x i64>` and similar shapes:

```llvm
define <2 x i64> @bc_pack_poison_lo() {
  ret <2 x i64> bitcast (<4 x i32> <i32 poison, i32 7,
                                    i32 9, i32 11> to <2 x i64>)
}
define <2 x i64> @bc_pack_poison_hi() {
  ret <2 x i64> bitcast (<4 x i32> <i32 5, i32 poison,
                                    i32 9, i32 11> to <2 x i64>)
}
```

```
$ opt -passes=instcombine -S -
define <2 x i64> @bc_pack_poison_lo() {
  ret <2 x i64> <i64 30064771072, i64 47244640265>   ; lane 0 should be poison
}                                                    ; (got 7<<32 with poison lane treated as 0)
define <2 x i64> @bc_pack_poison_hi() {
  ret <2 x i64> <i64 5, i64 47244640265>             ; lane 0 should be poison
}                                                    ; (got 5 with the high poison i32 treated as 0)
```

(`-passes=instsimplify` does not perform this fold; the wrong value
only appears after InstCombine, confirming the bug is in the constant
folder that InstCombine drives, not in some other pass.)

## What LangRef says

LangRef on poison:

> a poison value flowing into the operand of any instruction (including
> phi nodes) that depends on the value being defined results in the
> operand of the instruction itself becoming poison.

A `bitcast <N x iM>` to a scalar depends bit-for-bit on every source
lane.  A poison source lane therefore contaminates the entire
destination scalar with poison; the result must be `poison`, not a
specific integer.

The byte-vector path already encodes this rule (line 197-199).  The
fix is to mirror that rule for the integer/FP element path.

## Fix sketch

Add a poison check at the top of `foldConstVectorToAPInt`, or at the
caller in `FoldBitCast`, parallel to the existing
`SrcEltTy->isByteTy() && C->containsPoisonElement()` guard:

```cpp
// Bitcasting a vector containing any poison lane to an integer or fp
// type yields poison.  (Mirrors the isByteTy() guard below.)
if (C->containsPoisonElement())
  return PoisonValue::get(DestTy);
```

Or inside the loop, distinguish poison from undef:

```cpp
if (isa_and_nonnull<PoisonValue>(Element))
  return PoisonValue::get(/* wider scalar */);
if (isa_and_nonnull<UndefValue>(Element)) {
  Result <<= BitShift;
  continue;
}
```

Either guards the integer/FP source path and matches the byte-source
treatment.

## Why this matters at -O2

- The pattern `bitcast <N x scalar> to widerScalar` appears whenever a
  frontend or earlier optimization produces a SIMD-style aggregation
  (e.g., GCC-style bit-fields lowered into vectors, struct unpacking,
  SSE/AVX shuffles after vector legalization in IR form).
- A poison lane in such a constant is not unusual: it can arise from
  upstream UB-laundering folds (e.g. `add nsw INT_MAX, 1 -> poison`),
  shufflevector with poison mask elements, partially uninitialized
  aggregates, or `freeze`-removed sequences.
- The wrong fold quietly turns "poison" into a specific defined
  integer.  Subsequent passes then reason that the result is a known
  constant - so a `store` of it is fully defined, a `select` chooses a
  specific arm, etc.  This can:
  * Hide UB from sanitizers (UB-laundering), and
  * In principle, allow downstream passes to choose specific
    transformations contingent on the constant value (e.g.,
    branch-folding `icmp eq i64 %v, 5 -> true`) that would have been
    blocked by poison.

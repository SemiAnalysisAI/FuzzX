# howManyLessThans: unsigned LT vs AddRec RHS uses signed math + weak NoWrap gate

File: `llvm/lib/Analysis/ScalarEvolution.cpp`
Function: `ScalarEvolution::howManyLessThans`, lines ~13463–13505 (the
`!isLoopInvariant(RHS, L)` branch where RHS is an AddRec in the same loop with
a negative stride).

## The transform

When the latch test is `IV <p RHS` with both LHS and RHS being affine
recurrences in the same loop (IV step `Stride`, RHS step `RHSStride`) and
`RHSStride < 0`, SCEV produces:

```
BECount = ceil((End - Start) /u (Stride - RHSStride))
End     = max(RHSStart, Start)
```

The denominator `Stride - RHSStride` is the per-iteration unsigned closing
rate; this is the right formula **only if neither IV nor RHS wraps in the
domain matching the comparison**.

## What the code actually checks

Lines 13465–13504 (snippet):

```cpp
if (PositiveStride && RHSAddRec != nullptr && RHSAddRec->getLoop() == L &&
    any(RHSAddRec->getNoWrapFlags())) {                          // (A)
  const SCEV *RHSStart  = RHSAddRec->getStart();
  const SCEV *RHSStride = RHSAddRec->getStepRecurrence(*this);

  // Check if RHSStride < 0 and Stride - RHSStride will not overflow.
  if (isKnownNegative(RHSStride) &&                              // (B)
      willNotOverflow(Instruction::Sub, /*Signed=*/true, Stride, // (C)
                      RHSStride)) {
    const SCEV *Denominator = getMinusSCEV(Stride, RHSStride);
    if (isKnownPositive(Denominator)) {                          // (D)
      End = IsSigned ? getSMaxExpr(RHSStart, Start)
                     : getUMaxExpr(RHSStart, Start);
      const SCEV *Delta = getMinusSCEV(End, Start);
      BECount = getUDivCeilSCEV(Delta, Denominator);
      BECountIfBackedgeTaken =
          getUDivCeilSCEV(getMinusSCEV(RHSStart, Start), Denominator);
    }
  }
}
```

All of `PositiveStride`, `isKnownNegative`, `isKnownPositive` are **signed**
predicates (they consult `getSignedRangeMin/Max`; see lines 11239–11252:
`isKnownNegative(S) := getSignedRangeMax(S).isNegative()`).
`willNotOverflow(..., /*Signed=*/true, ...)` (C) also checks **signed**
no-overflow.

The bug pattern in the brief is "howManyLessThans for unsigned that uses
signed math". This block hits it three ways when `IsSigned == false`:

1. **(A) NoWrap gate is too weak.** `any(getNoWrapFlags())` accepts NSW alone,
   but for an unsigned comparison the relevant guarantee on the right side is
   NUW. With only NSW on `RHSAddRec`, RHS can perfectly well wrap unsignedly.
   Concretely, `{1, +, -1}<nsw>` from `i32 1` decreases signedly to `INT_MIN`,
   then *signed*-overflows to `INT_MAX` — but in *unsigned* terms it goes
   `1 → 0 → UINT_MAX → UINT_MAX-1 → …`, i.e. one step where RHS jumps from a
   small unsigned value to a huge one. The whole "closing-rate" model breaks.

2. **(B) `isKnownNegative(RHSStride)` is a signed test.** A SCEV whose
   signed-range-max is negative is not the same as "RHS decreases unsignedly
   each iteration". For unsigned LT we actually need to know RHS strictly
   decreases unsignedly per step, i.e. either `RHSStride` is a constant
   negative literal *and* RHS has NUW (so the subtraction does not wrap below
   0), or `RHSStride` is loop-invariant and an unsigned-wrap-free decrement
   on RHS is provable.

3. **(C) `willNotOverflow(Sub, Signed=true, Stride, RHSStride)` is signed.**
   The denominator is then used in a `/u` (unsigned-ceil divide). If
   `Stride = 1` and `RHSStride = INT_MIN` (signed negative, fine for
   `isKnownNegative`), `Stride - RHSStride = 1 - INT_MIN`, which is exactly
   the case where signed subtraction overflows… but the *unsigned* subtraction
   yields `1 - 0x80000000 = 0x80000001`, a perfectly valid unsigned positive
   value that would *not* be rejected by an unsigned overflow check. The
   signed check here is the wrong polarity for the consumer (which uses the
   value as an unsigned divisor).

The signed (D) `isKnownPositive(Denominator)` similarly: a SCEV that is signed
non-negative but extremely large (e.g. has signed-range-min = 0 but unsigned
range up to UINT_MAX) is fine, but if signed-range-min is `INT_MIN` because
SCEV cannot prove otherwise, this gate fails even when the unsigned divisor
is small and positive. (This direction is just lost optimization, not a
miscompile.)

## Why miscompile is plausible

Combine (A) + (B): pick an NSW-only RHS recurrence whose signed step is
negative but whose unsigned trajectory wraps inside the loop body. Then
`isKnownNegative` is true, `willNotOverflow(Sub, Signed=true, 1, RHSStride)`
is true for a `RHSStride` like `-1`, and (A) passes with just NSW. SCEV will
hand back a `BECount = ceil((max(RHSStart,Start) - Start) / (Stride -
RHSStride))` formula that is *unsigned*ly wrong, because in reality the loop
exits *much* sooner (RHS unsignedly underflows to a huge value, making
`IV <u RHS` true forever until IV overflows or wraps too). Downstream loop
unrolling / IndVarSimplify can then either unroll a wrong number of times or
delete the latch entirely.

## How to reproduce-hunt

Search corpus IR for:

* Latch of form `%cond = icmp ult i32 %iv, %rhs.iv` where both are PHIs.
* `%iv` is e.g. `add nuw i32 %iv, 1`.
* `%rhs.iv` is `add nsw i32 %rhs.iv, -1` (NSW only, *no* NUW).
* The loop has `mustprogress` and a single exiting block, with no other
  blocking analysis (e.g. assume()s) on the RHS unsigned wrap.

Run `opt -passes=indvars,loop-unroll` and inspect whether the unrolled trip
count matches the runtime behavior (compare against `-O0`).

## Why this is in-scope for x86

`indvars`/SCEV miscompiles surface at `llc` time as wrong trip counts in
unrolled / vectorized loops; the x86 backend has nothing to do with the
faulty analysis, but the wrong-IR output is the user-visible miscompile.

## Status: source-confirmed (signed predicates used in unsigned consumer).
Needs a fuzz-IR repro to confirm the (A)+(B)+(C) corner case actually
triggers in a way that wraps the unsigned-divide formula.

# w60: `fp_round(fp_extend x)` -> `x` elides IEEE sNaN-quieting

**File:lines:** `llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp:20235-20237`
(`DAGCombiner::visitFP_ROUND`)

## Reasoning

```cpp
// fold (fp_round (fp_extend x)) -> x
if (N0.getOpcode() == ISD::FP_EXTEND && VT == N0.getOperand(0).getValueType())
  return N0.getOperand(0);
```

This is dead-code-elimination for the round-trip pattern, but the round-trip
is *not* a no-op for signaling NaNs. Per IEEE 754-2019 §5.4.2
("convertFormat" operation), every format-conversion operation is a real
arithmetic operation that quiets sNaN inputs. So:

- `fpext sNaN to double`  -> qNaN (with payload zero-padded to the wider mantissa)
- `fptrunc qNaN to float` -> qNaN (with payload truncated)

The IR pair therefore is `sNaN -> qNaN`. The fold returns the original sNaN,
preserving the signaling bit. On x86 the round-trip lowers to
`cvtss2sd`/`cvtsd2ss` which on hardware quiet sNaN (and raise the
invalid-operation exception). The fold removes both conversions, so the
optimized program differs from the hardware behavior.

There is no flag check (`nnan` etc.) gating this fold, so it fires
unconditionally even when the IR carries no fast-math flags whatsoever.

User idiom: `x = (float)(double)x;` is a well-known C trick to quiet an sNaN
or to force a denormal round-trip. This fold silently turns the idiom into a
no-op.

## Candidate IR

```ll
target triple = "x86_64-unknown-linux-gnu"

define float @snan_round_trip(float %x) {
  %ext = fpext float %x to double
  %trunc = fptrunc double %ext to float
  ret float %trunc
}
```

## llc -O2 -mtriple=x86_64-- output

```asm
snan_round_trip:                        # @snan_round_trip
        retq
```

`%xmm0` is returned verbatim. If the caller passes an sNaN like
`0x7FA00000`, the function returns `0x7FA00000`. The reference behavior
(achieved by inlining the same pair via inline-asm to defeat the fold) is
`cvtss2sd; cvtsd2ss` which on x86 turns `0x7FA00000` into `0x7FE00000`
(qNaN) and raises invalid-operation.

## Why it matters

This is a sibling of the known DAGCombiner sNaN-quieting bypasses
(w12 / w60 simplifyFPBinop identity folds). The `fp_round(fp_extend x) -> x`
pattern is one of the most common round-trip idioms in user code (intentional
quieting, denormal round-trip, or `(float)(double)x` casts in templates), and
the fold has no FMF guard at all.

Fix: gate the fold on `(N->getFlags().hasNoNaNs() ||
N0->getFlags().hasNoNaNs() || !canBeSignalingNaN(input))`. Alternatively,
peek through to see whether the input is provably non-sNaN.

# w60: `simplifyFPBinop` `X + -0.0 -> X` and `X - +0.0 -> X` bypass sNaN quieting (sibling of w53)

**File:lines:** `llvm/lib/CodeGen/SelectionDAG/SelectionDAG.cpp:11605-11613`
(`SelectionDAG::simplifyFPBinop`), the FADD/FSUB arms specifically. The FMUL
and FDIV arms (lines 11615-11619) are already filed as w53 / bug-pending;
this candidate adds the previously-unfiled additive sibling identities.

## Reasoning

```cpp
// X + -0.0 --> X
if (Opcode == ISD::FADD)
  if (YC->getValueAPF().isNegZero())
    return X;

// X - +0.0 --> X
if (Opcode == ISD::FSUB)
  if (YC->getValueAPF().isPosZero())
    return X;
```

These are unguarded by any FMF flag — they fire on plain unflagged `fadd` /
`fsub`. For an `X` that is signaling-NaN, the IEEE-754 §6.2 behavior of
"add zero" / "subtract zero" is *not* a no-op: the operation quiets the
signaling input (sNaN→qNaN, mantissa MSB set) and raises the invalid
exception. The fold returns the original sNaN operand verbatim, so the
optimized program leaks the signaling bit while the unoptimized program
(or `-O0`) emits `addss`/`subss` against the zero constant and quiets.

This sister-pair extends the w53 / bug-035 / bug-002 chain of sNaN-quieting
bypasses already documented. The `+ -0.0` and `- +0.0` constants are exactly
the additive identities chosen so the *finite arithmetic* is a no-op, but the
*IEEE-NaN handling* of the corresponding hardware op still has to quiet.

Real user idiom: `x = x - 0.0f;` is a well-known C trick to quiet an sNaN
without changing finite values; this is suggested in compiler-explainer
articles and in some embedded toolchains. The DAGCombiner fold silently
destroys it. Equally common: `x + 0.0f` to force a `+0.0` from `-0.0` (the
plus-positive-zero direction has `isNegZero()` false so the fold doesn't
apply there, but `+ -0.0` does fire — and templates / generic numerics that
spell additive identity as `-0.0` to also coalesce `-0.0 + -0.0 -> -0.0`
hit it).

## Candidate IR

```ll
target triple = "x86_64-unknown-linux-gnu"

define float @fadd_neg0(float %x) {
  %r = fadd float %x, -0.0
  ret float %r
}

define float @fsub_pos0(float %x) {
  %r = fsub float %x, 0.0
  ret float %r
}

define double @fadd_neg0_d(double %x) {
  %r = fadd double %x, -0.0
  ret double %r
}

define <4 x float> @vfadd_neg0(<4 x float> %x) {
  %r = fadd <4 x float> %x, <float -0.0, float -0.0, float -0.0, float -0.0>
  ret <4 x float> %r
}
```

## `llc -O2 -mtriple=x86_64-unknown-linux-gnu` output

All four functions degenerate to a single `retq` (`%xmm0` is returned
verbatim):

```asm
fadd_neg0:                              # @fadd_neg0
        retq
fsub_pos0:                              # @fsub_pos0
        retq
fadd_neg0_d:                            # @fadd_neg0_d
        retq
vfadd_neg0:                             # @vfadd_neg0
        retq
```

Pass an sNaN bit pattern (`0x7FA00000` / `0x7FF4000000000000`) and it leaves
the function unchanged. Reference behavior on x86 (`addss`/`subss` against
the constant pool zero) quiets to `0x7FE00000` / `0x7FFC000000000000` and
sets the invalid-operation flag in MXCSR.

## Relationship to existing bugs

- **w53** (candidates): covers the FMUL/FDIV arms (`X * 1.0`, `X / 1.0`) of
  the *same* `simplifyFPBinop` function (lines 11615-11619). This candidate
  documents the *additive* arms (lines 11605-11613).
- **bug 002** (`fminimumnum`): same root pattern (identity fold ignoring
  NaN-quieting), different opcode.
- **bug 035** (`fmul X, -1.0 -> fsub -0.0, X -> fneg X`): different chain,
  same end-effect (sign-bit-only manipulation in place of full FP op).
- **bug 062** (`fsub -0.0, X -> fneg X`): direct ancestor; same NaN-sign-bit
  failure mode.
- **bug 112** (`fp_round(fp_extend x) -> x`): another sNaN-quieting fold
  bypass, different opcode pair.

Fix: gate FADD/FSUB arms on `(Flags.hasNoNaNs() ||
!operandCanBeSignalingNaN(X))`, mirroring the existing nnan/nsz check on
`X * 0.0 -> 0.0` (line 11622).

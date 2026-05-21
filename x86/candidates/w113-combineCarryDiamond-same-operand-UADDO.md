# Candidate: combineCarryDiamond picks wrong carry-in for UADDO(a,a) degenerate case

File: llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp:3842-3911

## Source pattern (lines 3867-3877)

```cpp
  // Check if nodes are connected in expected way.
  if (Carry1.getOperand(0) != Carry0.getValue(0) &&
      Carry1.getOperand(1) != Carry0.getValue(0))
    return SDValue();

  // The carry in value must be on the righthand side for subtraction.
  unsigned CarryInOperandNum =
      Carry1.getOperand(0) == Carry0.getValue(0) ? 1 : 0;
  if (Opcode == ISD::USUBO && CarryInOperandNum != 1)
    return SDValue();
  SDValue CarryIn = Carry1.getOperand(CarryInOperandNum);
```

The combiner extracts the "carry-in" of the second UADDO/USUBO as the
operand that is NOT `Carry0`'s sum result. The logic uses a simple
`Carry1.getOperand(0) == Carry0.getValue(0) ? 1 : 0` selector. This breaks
silently when both operands of `Carry1` are `Carry0.getValue(0)`:

- For UADDO carry-diamond `uaddo(uaddo(a,b), uaddo(a,b))` (i.e. the same
  intermediate sum fed into both lanes of an UADDO), `CarryInOperandNum`
  resolves to `1`, and we treat operand 1 (which is the prior sum itself,
  NOT a carry bit) as the carry-in.
- The subsequent `getAsCarry(TLI, CarryIn, true)` call (line 3884) is the
  only guard against using a non-carry value here. If `getAsCarry` accepts
  the sum because it happens to look like a boolean (e.g. legalisation
  zero-extended a single-bit value), we synthesize a UADDO_CARRY using
  the sum-of-A-and-B as the carry-in.

The shape `Carry1 = uaddo(SumAB, SumAB)` is degenerate but reachable when
a frontend doubles a UADDO sum (e.g. `x + x` after a `uaddo`); the
combiner does not reject it, and the resulting UADDO_CARRY is wrong
(the "carry-in" is a full-width add result, not a one-bit carry).

## Recommended fix

After computing `CarryInOperandNum`, reject the case where the *other*
operand of `Carry1` is *also* equal to `Carry0.getValue(0)` — i.e.
require exactly one operand of `Carry1` to be the prior sum:

```cpp
SDValue Other = Carry1.getOperand(1 - CarryInOperandNum);
if (Other != Carry0.getValue(0))
  return SDValue();  // The "non-carry" arg must be the prior sum.
```

(Equivalently: bail out when `Carry1.getOperand(0) == Carry1.getOperand(1)`.)

## Why this is a latent bug (asm-diff not yet reproduced)

Triggering UADDO_CARRY emission requires the target to legalize
USUBO_CARRY / UADDO_CARRY (line 3880 guard) AND `getAsCarry(..., true)`
(line 3884) to accept the full-width sum as a carry candidate. On x86_64
the legalization is satisfied; the open question is whether `getAsCarry`
ever passes a non-boolean through. Worker time-boxed.

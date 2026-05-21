# w600 — ConstantHoisting: cast-clone path preserves poison-generating flags / metadata

## Target
- `llvm/lib/Transforms/Scalar/ConstantHoisting.cpp:768-787`
- specifically `ClonedCastInst = CastInst->clone();` at line 774.

## Mechanism

`ConstantHoistingPass::collectConstantCandidates(Instruction *Inst, unsigned Idx)`
(lines 451-494) treats a cast-of-constant the same as the underlying
constant being used directly by `Inst`:

```
462    // Visit cast instructions that have constant integers.
463    if (auto CastInst = dyn_cast<Instruction>(Opnd)) {
464      // Only visit cast instructions, which have been skipped. All other
465      // instructions should have already been visited.
466      if (!CastInst->isCast())
467        return;
468      if (auto *ConstInt = dyn_cast<ConstantInt>(CastInst->getOperand(0))) {
469        // Pretend the constant is directly used by the instruction and ignore
470        // the cast instruction.
471        collectConstantCandidates(ConstCandMap, Inst, Idx, ConstInt);
472        return;
473      }
474    }
```

Later, in `emitBaseConstants(Base, Adj)` (lines 728-816), when the user's
operand is a *cast instruction* the pass clones the cast and reroutes its
operand to the materialized `Mat = Base + Offset`:

```
767    // Visit cast instruction.
768    if (auto CastInst = dyn_cast<Instruction>(Opnd)) {
769      assert(CastInst->isCast() && "Expected an cast instruction!");
770      Instruction *&ClonedCastInst = ClonedCastMap[CastInst];
771      if (!ClonedCastInst) {
774        ClonedCastInst = CastInst->clone();
775        ClonedCastInst->setOperand(0, Mat);
776        ClonedCastInst->insertAfter(CastInst->getIterator());
777        // Use the same debug location as the original cast instruction.
778        ClonedCastInst->setDebugLoc(CastInst->getDebugLoc());
```

`Instruction::clone()` copies *all* metadata and poison-generating flags
(`nneg`, `nuw`, `nsw`, `inbounds`, `disjoint`, etc.).  The pass deliberately
only overrides the `DebugLoc`.  No `dropPoisonGeneratingFlags()` /
`dropUBImplyingAttrs()` / `dropPoisonGeneratingMetadata()` is called on
`ClonedCastInst`.

The original cast carried those flags because *the original constant
value* satisfied the implied invariant (e.g. for `zext nneg i32 0x40 to
i64`, the i32 value's high bit is 0).  After cloning, the operand is
`Mat = Base + Offset` and the optimizer has *not* re-proven the invariant
for Mat.

## Why my repro does not break a current pipeline

Reproduced cloning with `trunc nuw` in the IR below
(`x86/candidates/w600-ch-cast-clone.ll`).  In the cloned IR the `trunc nuw`
flag is preserved.

```
$ opt -S -passes=consthoist w600-ch-cast-clone.ll
...
  %const = bitcast i64 4503599627370496 to i64
  %0 = trunc nuw i64 %const to i32
  %const_mat = add i64 %const, 1
  %1 = trunc nuw i64 %const_mat to i32
...
```

The diff against the input is a `trunc nuw` whose source operand
was changed from a ConstantInt to a `%const_mat` SSA value.  The
`nuw` flag survived intact.

In the *current* x86 cost model, within-group rebases of `trunc nuw` /
`zext nneg` constants are always to a base+offset that equals one of the
original (already-valid) constants modulo 2^N, so the flag remains
correct in this *particular* construction.  However, the code path makes
no such guarantee at the source level: if a target's `isLegalAddImmediate`
ever lets two constants group across a 2^N boundary, or a later refactor
changes the rebase arithmetic to operate in a wider type, the preserved
flags become latent UB.

In addition, `clone()` preserves arbitrary attached metadata
(`!nonnull`, `!range`, `!noundef`, `!dereferenceable`, `!align`,
`!noalias`, custom AA metadata).  None of these are necessarily true of
the new Mat operand.

## Suggested fix

Right after `ClonedCastInst = CastInst->clone();`, drop the
poison-generating flags and any value-range metadata that the rebased
operand may invalidate:

```cpp
ClonedCastInst->dropPoisonGeneratingFlags();
ClonedCastInst->dropPoisonGeneratingMetadata();
```

(`dropUBImplyingAttrs()` is for CallBase; not applicable here.)

## Files
- `x86/candidates/w600-ch-cast-clone.ll`

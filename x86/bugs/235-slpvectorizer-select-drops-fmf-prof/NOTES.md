# 235 — SLPVectorizer `Instruction::Select` codegen drops FMF, `!prof`, `!unpredictable`

Component: `llvm/lib/Transforms/Vectorize/SLPVectorizer.cpp` lines ~22981-23022 (Select case)

The Select case calls neither `propagateIRFlags` nor `::propagateMetadata`. The sibling `FNeg` case immediately below does both correctly — clear oversight.

4 lanes of `select nnan i1 %c, float ...` get merged into one `<4 x i1>` select with no `nnan`. Same for `!prof` (intentionally hard-coded via `CreateSelectWithUnknownProfile`) and `!unpredictable`.

## Reproducer

`opt -passes=slp-vectorizer -S repro.ll` (or `-O2 -S`):

Input: 4 scalar `select nnan` ops. Output: `%s = select <4 x i1> %m, <4 x float> %x, <4 x float> %y` — `nnan` dropped.

## Severity

Default x86 -O2. Loss of FMF prevents downstream FP optimizations on vectorized selects.

## Fix

After `Builder.CreateSelect`, add `cast<SelectInst>(Sel)->copyFastMathFlags(<first scalar>); ::propagateIRFlags(Sel, ScalarOps); ::propagateMetadata(Sel, ScalarOps);`.

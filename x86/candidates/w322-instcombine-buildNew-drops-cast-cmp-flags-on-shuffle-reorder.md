# InstCombine buildNew (shuffle reorder helper) drops cast/cmp flags

File: llvm/lib/Transforms/InstCombine/InstCombineVectorOps.cpp:1962-2036 (buildNew)
File: llvm/lib/Transforms/InstCombine/InstCombineVectorOps.cpp:3034-3037 (caller)

## Reasoning

`evaluateInDifferentElementOrder` is invoked from `visitShuffleVectorInst` at
the end:

```cpp
if (match(RHS, m_Poison()) && canEvaluateShuffled(LHS, Mask)) {
  Value *V = evaluateInDifferentElementOrder(LHS, Mask, Builder);
  return replaceInstUsesWith(SVI, V);
}
```

It walks a single-use expression tree and rebuilds it with reordered operands
via `buildNew` (line 1962). For binary operators, `buildNew` *does* propagate
`nuw`/`nsw`/`exact` and FMF (lines 1988-1998). But for the other rebuild paths
it does not:

- **ICmp** (line 2001-2004): `Builder.CreateICmp(...)` does not propagate the
  `samesign` flag from the original ICmp.
- **FCmp** (line 2005-2008): `Builder.CreateFCmp(...)` does not propagate FMF.
- **Cast ops** (line 2009-2026): `Builder.CreateCast(...)` does not propagate
  `zext nneg`, `trunc nuw`/`nsw`, or any other cast flag.
- **GEP** (line 2027-2032): does propagate `getNoWrapFlags()` already (good).

All four "lost" flags are pure refinements (they assert additional things about
the result) and dropping them produces correct but suboptimal IR.

## Reproducer

```llvm
target triple = "x86_64-unknown-linux-gnu"

define <4 x i1> @test_fcmp(float %a, float %b, float %c, float %d) {
  %i0 = insertelement <4 x float> poison, float %a, i32 0
  %i1 = insertelement <4 x float> %i0,    float %b, i32 1
  %i2 = insertelement <4 x float> %i1,    float %c, i32 2
  %i3 = insertelement <4 x float> %i2,    float %d, i32 3
  %cmp = fcmp nnan ninf olt <4 x float> %i3,
              <float 1.0, float 2.0, float 3.0, float 4.0>
  %r = shufflevector <4 x i1> %cmp, <4 x i1> poison,
                     <4 x i32> <i32 3, i32 2, i32 1, i32 0>
  ret <4 x i1> %r
}

define <4 x i32> @test_zext_nneg(i8 %a, i8 %b, i8 %c, i8 %d) {
  %i0 = insertelement <4 x i8> poison, i8 %a, i32 0
  %i1 = insertelement <4 x i8> %i0,    i8 %b, i32 1
  %i2 = insertelement <4 x i8> %i1,    i8 %c, i32 2
  %i3 = insertelement <4 x i8> %i2,    i8 %d, i32 3
  %z = zext nneg <4 x i8> %i3 to <4 x i32>
  %r = shufflevector <4 x i32> %z, <4 x i32> poison,
                     <4 x i32> <i32 3, i32 2, i32 1, i32 0>
  ret <4 x i32> %r
}

define <4 x i8> @test_trunc_nuw(i32 %a, i32 %b, i32 %c, i32 %d) {
  %i0 = insertelement <4 x i32> poison, i32 %a, i32 0
  %i1 = insertelement <4 x i32> %i0,    i32 %b, i32 1
  %i2 = insertelement <4 x i32> %i1,    i32 %c, i32 2
  %i3 = insertelement <4 x i32> %i2,    i32 %d, i32 3
  %t = trunc nuw <4 x i32> %i3 to <4 x i8>
  %r = shufflevector <4 x i8> %t, <4 x i8> poison,
                     <4 x i32> <i32 3, i32 2, i32 1, i32 0>
  ret <4 x i8> %r
}
```

`opt -passes=instcombine -S` produces (relevant lines):

```
%r = fcmp olt <4 x float> %4, <... 4.0, 3.0, 2.0, 1.0>      ; nnan ninf dropped
%r = zext <4 x i8> %4 to <4 x i32>                          ; nneg dropped
%r = trunc <4 x i32> %4 to <4 x i8>                         ; nuw  dropped
```

The reordered values are otherwise identical to the originals (per-lane), so
every flag that held before the reorder still holds after; the fold has no
reason to drop them.

## Fix sketch

```cpp
// Replace bare CreateICmp / CreateFCmp / CreateCast with copies that
// inherit IR flags from the original instruction:

case Instruction::ICmp: {
  Value *V = Builder.CreateICmp(...);
  if (auto *NewI = dyn_cast<Instruction>(V))
    NewI->copyIRFlags(I);
  return V;
}
case Instruction::FCmp: {
  Value *V = Builder.CreateFCmp(...);
  if (auto *NewI = dyn_cast<Instruction>(V))
    NewI->copyIRFlags(I);     // covers FMF
  return V;
}
case Instruction::Trunc: case Instruction::ZExt: ... {
  Value *V = Builder.CreateCast(...);
  if (auto *NewI = dyn_cast<Instruction>(V))
    NewI->copyIRFlags(I);     // covers nneg, trunc nuw/nsw
  return V;
}
```

`Instruction::copyIRFlags` (Instruction.cpp:721-763) already handles
`PossiblyNonNegInst`, `TruncInst` flags, FMF, and `ICmpInst::SameSign`, so the
single-line call is the entire fix per case.

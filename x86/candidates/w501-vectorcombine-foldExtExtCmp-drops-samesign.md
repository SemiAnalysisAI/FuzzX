# w501 - VectorCombine `foldExtExtCmp` drops `samesign` on icmp

## Location

`llvm/lib/Transforms/Vectorize/VectorCombine.cpp`

- Helper: `VectorCombine::foldExtExtCmp` line 610
- Caller: `VectorCombine::foldExtractExtract` line 644 (cmp branch, line 704)

```cpp
// line 617-619
CmpInst::Predicate Pred = cast<CmpInst>(&I)->getPredicate();
Value *VecCmp = Builder.CreateCmp(Pred, V0, V1);
return Builder.CreateExtractElement(VecCmp, ExtIndex, "foldExtExtCmp");
```

Two issues converge to drop `samesign`:

1. `cast<CmpInst>(&I)->getPredicate()` returns `CmpInst::Predicate` —
   the raw enum that does NOT carry the `samesign` bit. (The carrier
   type is `CmpPredicate`, obtained via `ICmpInst::getCmpPredicate()`
   or by checking `hasSameSign()`.)
2. There is no `copyIRFlags` call on `VecCmp` after construction.

Compare the sibling helper `foldExtExtBinop` at line 625 which
explicitly calls `VecBOInst->copyIRFlags(&I)` at line 638 to preserve
flags on the produced binop. The cmp helper skipped the equivalent
step.

The output is a strict weakening: `icmp samesign` becomes plain `icmp`.

## Repro (`repro.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

define i1 @samesign_extextcmp(<4 x i32> %x, <4 x i32> %y) {
  %e0 = extractelement <4 x i32> %x, i32 0
  %e1 = extractelement <4 x i32> %y, i32 0
  %c  = icmp samesign slt i32 %e0, %e1
  ret i1 %c
}
```

## Invocation

```
opt -mtriple=x86_64-unknown-linux-gnu -passes=vector-combine -S repro.ll
```

## Observed `opt` output

```llvm
define i1 @samesign_extextcmp(<4 x i32> %x, <4 x i32> %y) {
  %1 = icmp slt <4 x i32> %x, %y       ; <-- samesign gone
  %c = extractelement <4 x i1> %1, i32 0
  ret i1 %c
}
```

Default x86 -O2 reproduces identically (`-O2 -S` yields the same
`icmp slt` without `samesign`).

## Why it matters

`samesign` is a value-tracking primitive: it tells later passes that
both operands of the comparison have identical sign. InstCombine and
ValueTracking-based folds rely on this to canonicalize signed/unsigned
predicates, fold to constants, or simplify range computations. Once
dropped here, the information cannot be recovered without re-deriving
it via slower KnownBits analysis on the vector form.

## Fix sketch

```cpp
Value *foldExtExtCmp(Value *V0, Value *V1, Value *ExtIndex, Instruction &I) {
  auto *CI = cast<CmpInst>(&I);
  Value *VecCmp = Builder.CreateCmp(CI->getPredicate(), V0, V1);
  // Mirror foldExtExtBinop: any flags on the original cmp are safe to
  // back-propagate because unused vector lanes are discarded by the extract.
  if (auto *VecCmpI = dyn_cast<Instruction>(VecCmp))
    VecCmpI->copyIRFlags(&I);
  return Builder.CreateExtractElement(VecCmp, ExtIndex, "foldExtExtCmp");
}
```

`copyIRFlags` already handles `samesign` (see `Instruction::copyIRFlags`
at `llvm/lib/IR/Instruction.cpp` line 762 / 807).

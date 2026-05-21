# ConstantFoldGetElementPtr: poison index folded to base pointer (loses poison)

## Summary

`ConstantFoldGetElementPtr` in `llvm/lib/IR/ConstantFold.cpp` treats a
PoisonValue index identically to a null/undef index in its "no-op" check.
The check uses

```cpp
return IdxC->isNullValue() || isa<UndefValue>(IdxC);
```

and because `class PoisonValue final : public UndefValue`, `isa<UndefValue>`
returns true for both undef and poison.  As a result a constant GEP whose
indices are exclusively zero/undef/**poison** is folded to the base
pointer.  Per LangRef ("a poison value flowing into the operand of any
instruction that depends on the value being defined results in the
operand of the instruction itself becoming poison") and the LangRef
example for GEP, the result of `getelementptr i32, ptr @h, i32 poison`
must be `poison`, not `@h`.

The contrast with `InstructionSimplify::simplifyGEPInst`, which
explicitly checks `any_of(Indices, IsaPred<PoisonValue>)` and returns
`PoisonValue::get(GEPTy)`, shows that the two paths disagree.  The
constant folder path is what InstCombine takes (via
`ConstantFoldInstruction` -> `ConstantFoldInstOperandsImpl` ->
`ConstantExpr::getGetElementPtr` -> `ConstantFoldGetElementPtr`), so the
bug is observable at `-passes=instcombine`.

## Source

`llvm/lib/IR/ConstantFold.cpp:1349-1380` (`ConstantFoldGetElementPtr`):

```cpp
1363    auto IsNoOp = [&]() {
1364      // Avoid losing inrange information.
1365      if (InRange)
1366        return false;
1367
1368      return all_of(Idxs, [](Value *Idx) {
1369        Constant *IdxC = cast<Constant>(Idx);
1370        return IdxC->isNullValue() || isa<UndefValue>(IdxC);   // <-- treats poison as 0
1371      });
1372    };
1373    if (IsNoOp())
1374      return GEPTy->isVectorTy() && !C->getType()->isVectorTy()
1375                 ? ConstantVector::getSplat(...)
1376                 : C;
```

Compare `llvm/lib/Analysis/InstructionSimplify.cpp:5259-5262`:

```cpp
5259    // getelementptr poison, idx -> poison
5260    // getelementptr baseptr, poison -> poison
5261    if (isa<PoisonValue>(Ptr) || any_of(Indices, IsaPred<PoisonValue>))
5262      return PoisonValue::get(GEPTy);
```

The InstSimplify path matches LangRef; the ConstantFold path does not.

`include/llvm/IR/Constants.h:1660` confirms the type hierarchy:
`class PoisonValue final : public UndefValue { ... };`.

## Reproducer (x86-64, default `-O2` pipeline)

`gep_poison.ll`:

```llvm
@A = external global [100 x i32]

; All-poison index: must yield poison
define ptr @gep_poison_idx() {
  ret ptr getelementptr inbounds (i32, ptr @A, i64 poison)
}

; Trailing poison after zeros: same issue
define ptr @gep_zero_then_poison() {
  ret ptr getelementptr inbounds ([100 x i32], ptr @A, i64 0, i64 poison)
}

; Runtime form: InstCombine constant-folds it to the same wrong result
define ptr @runtime_gep_poison() {
  %g = getelementptr inbounds i32, ptr @A, i64 poison
  ret ptr %g
}

; Poison created by upstream UB (add nsw INT_MAX, 1 -> poison) flows in
define ptr @runtime_gep_overflow_poison(i64 %x) {
  %p = add nsw i64 9223372036854775807, 1
  %g = getelementptr inbounds i32, ptr @A, i64 %p
  ret ptr %g
}
```

```
$ opt -passes=instcombine -S gep_poison.ll
define ptr @gep_poison_idx() {
  ret ptr @A                      ; wrong: should be poison
}
define ptr @gep_zero_then_poison() {
  ret ptr @A                      ; wrong: should be poison
}
define ptr @runtime_gep_poison() {
  ret ptr @A                      ; wrong: should be poison
}
define ptr @runtime_gep_overflow_poison(i64 %x) {
  ret ptr @A                      ; wrong: should be poison
}
```

For comparison, instsimplify alone produces the right answer on the
runtime form:

```
$ opt -passes=instsimplify -S gep_poison.ll
define ptr @runtime_gep_poison() {
  ret ptr poison                  ; correct
}
```

So the two passes disagree about the same input, with the constant-fold
path silently strengthening poison into a defined value (@A).  In default
`-O2` the InstCombine result wins, so a downstream consumer that depended
on `poison` (e.g. a `freeze` followed by reasoning that the value
escaped, or a later optimization that exploits poison propagation)
loses information.  More concretely, this is unsound replacement of
`poison` by a defined pointer: a downstream `store i32 0, ptr %g` is no
longer UB on the second/fourth functions above.

## Fix sketch

Either:

1. In `IsNoOp`, replace `isa<UndefValue>(IdxC)` with
   `isa<UndefValue>(IdxC) && !isa<PoisonValue>(IdxC)`, **and** add an
   explicit early-return that mirrors InstSimplify:

   ```cpp
   if (any_of(Idxs, [](Value *V) { return isa<PoisonValue>(V); }))
     return PoisonValue::get(GEPTy);
   ```

   (also handle the base-pointer case at line 1357 for completeness if
   any_of is preferred over a separate check).

2. Or, more conservatively, just emit the poison check *before* the
   `IsNoOp` shortcut.

The first option also matches existing per-instruction checks in
`ConstantFoldCastInstruction` (line 165), `ConstantFoldSelectInstruction`
(line 313/331/341/343), and `ConstantFoldExtractElementInstruction`
(line 379), all of which carefully separate poison from undef.

## Why this matters at -O2

- `-O2` runs InstCombine (and the SROA / GVN pipeline that often
  produces constant GEPs of poisoned indices via prior peephole folds,
  e.g. `add nsw INT_MAX, 1`).
- Downstream passes that rely on poison semantics (e.g. branch-on-poison
  is UB) can be miscompiled: a transformation that should be blocked
  because the value is poison may proceed.
- The bug also enables a UB-laundering pattern: `getelementptr inbounds
  T, ptr @G, i64 poison` becomes `@G`, and then `store i8 0, ptr ...`
  appears defined, hiding the upstream UB from sanitizers/static
  analyses that reason on the IR after InstCombine.

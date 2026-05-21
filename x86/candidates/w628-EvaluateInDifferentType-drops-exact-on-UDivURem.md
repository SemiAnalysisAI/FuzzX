# w628: EvaluateInDifferentType drops `exact` from `udiv`/`urem` (and nsw/nuw from arithmetic ops) when narrowing

## Source
- File: `llvm/lib/Transforms/InstCombine/InstCombineCasts.cpp`
- Function: `EvaluateInDifferentTypeImpl` (file-scope static)
- Lines: 55-75

## Code

```cpp
static Value *EvaluateInDifferentTypeImpl(Value *V, Type *Ty, bool isSigned,
                                          InstCombinerImpl &IC,
                                          EvaluatedMap &Processed) {
  ...
  switch (Opc) {
  case Instruction::Add:
  case Instruction::Sub:
  case Instruction::Mul:
  case Instruction::And:
  case Instruction::Or:
  case Instruction::Xor:
  case Instruction::AShr:
  case Instruction::LShr:
  case Instruction::Shl:
  case Instruction::UDiv:
  case Instruction::URem: {
    Value *LHS = EvaluateInDifferentTypeImpl(I->getOperand(0), Ty, isSigned, IC,
                                             Processed);
    Value *RHS = EvaluateInDifferentTypeImpl(I->getOperand(1), Ty, isSigned, IC,
                                             Processed);
    Res = BinaryOperator::Create((Instruction::BinaryOps)Opc, LHS, RHS);
    if (Opc == Instruction::LShr || Opc == Instruction::AShr)
      Res->setIsExact(I->isExact());     // <-- only LShr/AShr preserve exact
    break;
  }
```

Only `LShr` and `AShr` get their `exact` flag forwarded. Other exact-eligible
operations — `UDiv` and `URem` — silently lose the `exact` flag. `Add`, `Sub`,
`Mul`, `Shl` lose `nsw`/`nuw`.

## Repro: `/tmp/icc625/sdiv-exact.ll` / `trunc-udiv-exact.ll`

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i8 @t1(i8 %a, i8 %b) {
  %za = zext i8 %a to i32
  %zb = zext i8 %b to i32
  %d  = udiv exact i32 %za, %zb        ; exact
  %r  = trunc i32 %d to i8
  ret i8 %r
}
```

`opt -passes=instcombine -S`:

```llvm
define i8 @t1(i8 %a, i8 %b) {
  %1 = udiv i8 %a, %b                  ; exact lost
  ret i8 %1
}
```

## Analysis

`udiv exact A, B` guarantees `B` divides `A` exactly (otherwise poison).
Narrowing from a wider type to a narrower type when both operands have
zero upper bits is value-preserving — the narrow `udiv` produces the same
result for non-poison inputs. The `exact` flag would still hold in the
narrow type: the exactness condition (`A % B == 0`) is unchanged by the
narrowing because both operands fit in the narrow type.

For `Add/Sub/Mul/Shl` losing nsw/nuw: narrowing can change overflow
behavior, so dropping these flags is **necessary** (not a missed opt).
But for `UDiv exact`/`URem exact` and for `Shl` losing `nsw` when shift
amount is small enough, the loss is unforced.

## Severity

Soundness-preserving missed optimization. Dropping `exact` is always safe
(it removes a poison-generating condition). But downstream folds that
key off `udiv exact` (e.g. recognizing `x * (A udiv exact B) == A` as a
divisibility predicate) cannot fire.

## Fix sketch

```cpp
case Instruction::UDiv:
case Instruction::URem: {
  ...
  Res = BinaryOperator::Create((Instruction::BinaryOps)Opc, LHS, RHS);
  if (auto *PEO = dyn_cast<PossiblyExactOperator>(I))
    cast<BinaryOperator>(Res)->setIsExact(PEO->isExact());
  break;
}
```

(LShr/AShr already do this — should be unified.)

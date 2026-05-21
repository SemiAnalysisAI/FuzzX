# instsimplify: simplifySelectInst replaces vector select with a non-constant arm when condition has a poison lane, losing per-lane poison

- Layer: middle-end (InstSimplify / LLVM IR)
- Pass: `instsimplify` (Analysis/InstructionSimplify.cpp::simplifySelectInst)
- Architecture: target-independent; reproduces with x86 `-O2` and standalone `-passes=instsimplify`
- Severity: miscompile (silently replaces a result lane that must be poison with a defined value)
- LLVM HEAD tested: 23.0.0git, repo HEAD `0dd29960c`

## Summary

`simplifySelectInst` in `llvm/lib/Analysis/InstructionSimplify.cpp` looks at the
condition operand, and if it matches `m_One()` / `m_Zero()` / `m_Undef()`,
collapses the select to `TrueVal` or `FalseVal`. The matchers `m_One()`,
`m_Zero()`, and `m_Undef()` all accept *vector constants whose lanes are a
mixture of the target value and `poison`* (`AllowPoison=true` in
`cstval_pred_ty`; `m_Undef()` recursively accepts undef+poison aggregates).

The semantic problem: per LangRef, for a vector `select`, "if a condition
element is poison, the result element is poison". The simplifier ignores
this. Replacing the select with `TrueVal` (or `FalseVal`) means lanes whose
condition was `poison` now take their value from `TrueVal`/`FalseVal`. When
`TrueVal`/`FalseVal` is a non-constant value (function argument, computation,
etc.), those lanes become defined where they should have been poison. This is
a relaxation of poison and is a miscompile (an unconstrained downstream use is
now constrained).

The all-constant case is handled correctly by
`ConstantFoldSelectInstruction` in `lib/IR/ConstantFold.cpp` (it
inspects each lane and emits `poison` when the condition lane is poison —
see lines 312-314). The bug is only in the non-all-constant fast paths of
`simplifySelectInst`.

## Root cause (source citations)

`llvm/lib/Analysis/InstructionSimplify.cpp`, function `simplifySelectInst`:

- Line 5046-5048: `// select poison, X, Y -> poison`
  `if (isa<PoisonValue>(CondC)) return PoisonValue::get(TrueVal->getType());`
  -- only catches the all-poison `PoisonValue`. A `ConstantVector` like
  `<i1 true, i1 poison>` is *not* `isa<PoisonValue>`, so this case slips through.

- Line 5050-5052: `// select undef, X, Y -> X or Y`
  `if (Q.isUndefValue(CondC))`
  `  return isa<Constant>(FalseVal) ? FalseVal : TrueVal;`
  -- `Q.isUndefValue` uses `match(V, m_Undef())`, which accepts
  `ConstantVector` aggregates whose elements are undef *or poison*
  (`undef_match::checkAggregate` in `PatternMatch.h:149-167`). For
  `<undef, poison>` this returns the (non-constant) `TrueVal` whole, so the
  poison lane is silently filled with `TrueVal`'s lane 1.

- Line 5054-5061: `// select true, X, Y --> X` / `// select false, X, Y --> Y`
  `// For vectors, allow undef/poison elements in the condition to match the`
  `// defined elements, so we can eliminate the select.`
  `if (match(CondC, m_One()))  return TrueVal;`
  `if (match(CondC, m_Zero())) return FalseVal;`
  -- `m_One()` / `m_Zero()` use `cstval_pred_ty` with `AllowPoison=true`
  (`PatternMatch.h:317-358`), so `<i1 true, i1 poison>` matches `m_One()`.
  The comment explicitly acknowledges accepting poison lanes but does *not*
  account for the poison-propagation rule.

The `ConstantFoldSelectInstruction` path at line 5043 (`if all-constant`)
*does* handle lane-wise poison correctly, so the inconsistency is internal
to InstSimplify.

## Reproducer 1: `<true, poison>` cond, non-constant `TrueVal`

`/tmp/llvmtest/sel_one_poison.ll`:

```llvm
define i32 @one_poison(<2 x i32> %tv, <2 x i32> %fv) {
  %r = select <2 x i1> <i1 true, i1 poison>, <2 x i32> %tv, <2 x i32> %fv
  %e = extractelement <2 x i32> %r, i64 1
  ret i32 %e
}
```

`opt -passes=instsimplify -S` (LLVM HEAD `0dd29960c`) yields:

```llvm
define i32 @one_poison(<2 x i32> %tv, <2 x i32> %fv) {
  %e = extractelement <2 x i32> %tv, i64 1
  ret i32 %e
}
```

Correct semantics: lane 1 of `%r` is `poison` (cond lane is poison), so the
`extractelement` of lane 1 must produce `poison`. The optimizer instead
produced a defined load from `%tv`. This is a refinement violation:
poison -> a concrete value.

## Reproducer 2: `<false, poison>` cond, non-constant `FalseVal`

Same flavor, mirrored to the `m_Zero()` branch.

```llvm
define i32 @zero_poison(<2 x i32> %tv, <2 x i32> %fv) {
  %r = select <2 x i1> <i1 false, i1 poison>, <2 x i32> %tv, <2 x i32> %fv
  %e = extractelement <2 x i32> %r, i64 1
  ret i32 %e
}
```

`opt -passes=instsimplify -S`:

```llvm
define i32 @zero_poison(<2 x i32> %tv, <2 x i32> %fv) {
  %e = extractelement <2 x i32> %fv, i64 1
  ret i32 %e
}
```

## Reproducer 3: `<undef, poison>` cond (hits the `m_Undef()` path)

```llvm
define i32 @undef_poison(<2 x i32> %tv, <2 x i32> %fv) {
  %r = select <2 x i1> <i1 undef, i1 poison>, <2 x i32> %tv, <2 x i32> %fv
  %e = extractelement <2 x i32> %r, i64 1
  ret i32 %e
}
```

`opt -passes=instsimplify -S`:

```llvm
define i32 @undef_poison(<2 x i32> %tv, <2 x i32> %fv) {
  %e = extractelement <2 x i32> %tv, i64 1
  ret i32 %e
}
```

The cond is "Q.isUndefValue" (matches `m_Undef()` because it's an aggregate of
undef+poison), `FalseVal` is non-constant, so the simplifier returns
`TrueVal`. Lane 1's poison is lost.

## Reproducer 4: 4-lane variant proving it scales

```llvm
define <4 x i32> @sel_part(<4 x i32> %tv, <4 x i32> %fv) {
  %r = select <4 x i1> <i1 true, i1 poison, i1 true, i1 true>,
                <4 x i32> %tv, <4 x i32> %fv
  ret <4 x i32> %r
}
```

`opt -passes=instsimplify -S`:

```llvm
define <4 x i32> @sel_part(<4 x i32> %tv, <4 x i32> %fv) {
  ret <4 x i32> %tv
}
```

Lane 1 of the result is required to be `poison`, but the rewrite gives lane 1
the value of `%tv` lane 1.

## Contrast: the all-constant path is correct

When `TrueVal` and `FalseVal` are also constant, `ConstantFoldSelectInstruction`
walks the condition element-wise (see `lib/IR/ConstantFold.cpp:295-329`) and
emits `poison` for each poison-cond lane:

```llvm
define <2 x i32> @cfold() {
  %r = select <2 x i1> <i1 true, i1 poison>,
                <2 x i32> <i32 5, i32 6>, <2 x i32> <i32 7, i32 8>
  ret <2 x i32> %r
}
```

`opt -passes=instsimplify -S`:

```llvm
define <2 x i32> @cfold() {
  ret <2 x i32> <i32 5, i32 poison>   ; lane 1 correctly poison
}
```

This proves the spec is what I claim, and that the bug is the asymmetry between
`ConstantFoldSelectInstruction` (correct, per-lane) and the
`simplifySelectInst` fast paths (incorrect, whole-vector).

## Why it is a miscompile, not a benign optimisation

The transform takes a value that the language guarantees is `poison` and
replaces it with a defined value. The LangRef rule:

> If the condition is a vector ... If a condition element is poison, the
> result element is poison.

A poison lane is *more* undefined than any specific value (poison is the
universal refinement). Replacing poison with `tv[1]` constrains downstream
behaviour. In particular, any user of the lane that performs a side-effect
ordered by poison (UB on freezing, conditional branches with `br i1 poison`,
stores to poison addresses, etc.) is now executed on a defined input where it
previously could have been UB. The opposite direction (refining poison to a
concrete value) is the textbook miscompile signature.

The reproducer end-to-end at x86 `-O2`:

```
; ModuleID = '/tmp/llvmtest/sel_canonical.ll'
target triple = "x86_64-unknown-linux-gnu"
define i32 @use_poison_lane(<2 x i32> %tv, <2 x i32> %fv) {
  %r = select <2 x i1> <i1 true, i1 poison>, <2 x i32> %tv, <2 x i32> %fv
  %e = extractelement <2 x i32> %r, i64 1
  ret i32 %e
}
```

`opt -O2 -S` produces `ret i32 <extract of %tv lane 1>` with no `freeze`,
confirming the miscompile is observable on the default pipeline.

## Fix sketch

In `simplifySelectInst`, before each of the three early-exit paths
(`PoisonValue` cond, `m_Undef()` cond, `m_One()`/`m_Zero()` cond) for *vector*
conditions, only take the shortcut when:
- the condition has no poison element (use `Constant::containsPoisonElement()`
  / `getSplatValue(/*AllowPoison=*/false)`), or
- the chosen arm is itself a constant whose lanes corresponding to poison-cond
  lanes are also `poison`, or
- fall through and let `ConstantFoldSelectInstruction` do the per-lane work
  when all three operands are constant.

A simple guard at line 5046, 5050, and 5058 would be: if
`CondC->containsPoisonElement()` and the picked arm is not a Constant we can
mask appropriately, return `nullptr`. The all-poison case at line 5047 stays
correct (returning poison is fine). The shortcut path at line 5071
(`select i1 Cond, i1 true, i1 false --> Cond`) and onward are scalar i1 (not
vector) so unaffected.

## Hunt area cross-reference

This was the "simplifySelect with mixed undef/poison" item in the hunt brief.
The other items (NaN+0 in simplifyFMul/FAdd, FPExt fold, scalarizeBinOp FMF,
vectorizeLoadInsert metadata, scalarizeExtExtract `!range`) were probed and
either behaved correctly on HEAD (`fadd nnan undef -> poison`, `fadd nsz` does
not over-fold), are off by default on x86 (`scalarizeExtExtract` requires
`allowVectorElementIndexingUsingGEP`), or are conservative drops (load
metadata).

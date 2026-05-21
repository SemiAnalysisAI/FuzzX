# w410 — `ConstantFoldShuffleVectorInstruction` lowers `PoisonMaskElem` to `undef` instead of `poison`

## Component
`llvm/lib/IR/ConstantFold.cpp` — `llvm::ConstantFoldShuffleVectorInstruction`

## Source citation
`llvm/lib/IR/ConstantFold.cpp:479-535` (focus on lines **514-519**)

```cpp
  // Loop over the shuffle mask, evaluating each element.
  SmallVector<Constant*, 32> Result;
  for (unsigned i = 0; i != MaskNumElts; ++i) {
    int Elt = Mask[i];
    if (Elt == -1) {
      Result.push_back(UndefValue::get(EltTy));   // <-- BUG: should be PoisonValue
      continue;
    }
```

## Language reference
The header file `llvm/include/llvm/IR/Instructions.h:1949` is unambiguous:

```
/// PoisonMaskElem (-1) specifies that the result element is poison.
```

and `Instructions.h:1941`:
```cpp
constexpr int PoisonMaskElem = -1;
```

The mask sentinel value `-1` is named `PoisonMaskElem` exactly because a `-1` mask lane is required to evaluate to `poison`. The constant-folder, however, emits `undef` for that lane, leaking a strictly more "alive" value into the resulting `ConstantVector`.

Notice the inconsistency *within the same function*: lines 487-490 do the all-poison-mask short-circuit correctly:

```cpp
  // Poison shuffle mask -> poison value.
  if (all_of(Mask, equal_to(PoisonMaskElem))) {
    return PoisonValue::get(VectorType::get(EltTy, MaskEltCount));
  }
```

i.e. if every mask lane is the `PoisonMaskElem` sentinel, the function correctly returns a full-poison vector. If at least one mask lane is concrete, the loop is taken — and now the `-1` lanes silently become `undef`. So the folder produces `undef` or `poison` for the same input lane depending on what the *other* lanes happen to be.

## Reproducer (`/tmp/cf_hunt/shuffle_poison_mask.ll`)
```llvm
define <4 x i32> @test() {
  %r = shufflevector
        <4 x i32> <i32 10, i32 20, i32 30, i32 40>,
        <4 x i32> <i32 100, i32 200, i32 300, i32 400>,
        <4 x i32> <i32 0, i32 poison, i32 5, i32 poison>
  ret <4 x i32> %r
}
```

Command:
```
opt -passes=instsimplify -S /tmp/cf_hunt/shuffle_poison_mask.ll
```

## Actual output
```llvm
define <4 x i32> @test() {
  ret <4 x i32> <i32 10, i32 undef, i32 200, i32 undef>
}
```

## Expected output
```llvm
define <4 x i32> @test() {
  ret <4 x i32> <i32 10, i32 poison, i32 200, i32 poison>
}
```

(or, equivalently, anything whose lanes 1 and 3 are exactly `poison` — not the weaker `undef`).

## Why this matters
1. `undef` is a *strictly less* poisonous value than `poison`. Replacing `poison` with `undef` is a refinement violation in the wrong direction: code that was allowed to be UB on the user's program is now turned into well-defined nondeterminism, which can change later analysis outcomes (e.g. `freeze` folding, `select` simplification, range analysis, demanded-bits analysis treats `undef` and `poison` differently in some passes).
2. Round-trip identity is broken: an unfolded `shufflevector` with `<i32 poison>` in its mask, when its operands later become constant, decays to `undef`. Subsequent constant folding can then re-materialise *non-poison* values from what was originally a poison lane.
3. The same function gets it right when *all* mask lanes are `-1` (lines 487-490) — so the bug is purely the divergence between the short-circuit and the main loop. The fix is a one-token change at line 517 (`UndefValue` → `PoisonValue`), with the same one-token change advisable at line 522 for OOB mask lanes (`unsigned(Elt) >= SrcNumElts*2`), which similarly represent an "ill-formed" position that the IR semantics say is poison, not undef.

## Severity
Miscompilation-class semantic bug in constant folding: spec says **poison**, folder produces **undef**. Reaches the IR via `opt -passes=instsimplify` on the simplest possible inputs.

## Confidence
High. The contradiction is between the IR-level documentation (`Instructions.h:1949`, "PoisonMaskElem (-1) specifies that the result element is poison") and the implementation at `ConstantFold.cpp:517`, and the same function's own short-circuit path (`ConstantFold.cpp:489`) takes the documented behaviour. Reproducer is minimal and deterministic.

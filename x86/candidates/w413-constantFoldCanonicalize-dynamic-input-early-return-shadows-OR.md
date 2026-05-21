# w413 — `constantFoldCanonicalize` has dead code in its denormal-mode dispatcher

## Component
`llvm/lib/Analysis/ConstantFolding.cpp` — `constantFoldCanonicalize`

## Source citation
`llvm/lib/Analysis/ConstantFolding.cpp:2509-2533`:

```cpp
if (Src.isDenormal() && CI->getParent() && CI->getFunction()) {
  DenormalMode DenormMode =
      CI->getFunction()->getDenormalMode(Src.getSemantics());

  if (DenormMode == DenormalMode::getIEEE())
    return ConstantFP::get(CI->getContext(), Src);

  if (DenormMode.Input == DenormalMode::Dynamic)            //  <-- early bail #1
    return nullptr;

  // If we know if either input or output is flushed, we can fold.
  if ((DenormMode.Input == DenormalMode::Dynamic &&         //  <-- DEAD: Input==Dynamic already returned above
       DenormMode.Output == DenormalMode::IEEE) ||
      (DenormMode.Input == DenormalMode::IEEE &&
       DenormMode.Output == DenormalMode::Dynamic))
    return nullptr;

  bool IsPositive =
      (!Src.isNegative() || DenormMode.Input == DenormalMode::PositiveZero ||
       (DenormMode.Output == DenormalMode::PositiveZero &&
        DenormMode.Input == DenormalMode::IEEE));

  return ConstantFP::get(CI->getContext(),
                         APFloat::getZero(Src.getSemantics(), !IsPositive));
}
```

## The defect
The early bail-out at **line 2516-2517** (`if (DenormMode.Input == DenormalMode::Dynamic) return nullptr;`) makes the first alternative inside the OR at **line 2520-2521** (`DenormMode.Input == DenormalMode::Dynamic && DenormMode.Output == DenormalMode::IEEE`) provably unreachable: by the time control reaches the OR, `DenormMode.Input` is not `Dynamic`.

The OR is therefore equivalent to just its second alternative:
```cpp
if (DenormMode.Input == DenormalMode::IEEE &&
    DenormMode.Output == DenormalMode::Dynamic)
  return nullptr;
```

The fact that the original author wrote a two-arm OR strongly suggests they intended to cover *both* "Output is dynamic" cases (one with `Input == Dynamic`, one with `Input == IEEE`). The earlier bail-out at line 2516-2517 has rendered half of that intent dead.

## Why this matters
There is a semantic coverage gap left behind by the dead branch: when `DenormMode.Output == DenormalMode::Dynamic` is paired with `DenormMode.Input` of `PreserveSign` or `PositiveZero`, neither bail-out fires. Control falls through to the `IsPositive` computation at lines 2526-2529, which derives a sign-bit purely from the *input* denormal mode without consulting `Output` (other than the specific `Output == PositiveZero && Input == IEEE` arm, which the OR above just excluded for `Output == Dynamic`).

This means the folder picks a definite sign for a flushed-to-zero result while the runtime denormal-output mode is `Dynamic` — i.e. while the runtime might *not* flush at all (it might preserve the denormal). Committing to "flush, sign = X" ahead of time defeats the runtime configurability that `Dynamic` is meant to preserve.

(In practice the worst-case interaction is hard to trigger because the LLVM IR parser does not surface the `denormal-fp-math` string attribute via `Attribute::DenormalFPEnv`, and so this fall-through is currently shadowed by the function-attribute-plumbing — see notes below. The dead-code observation is independent of any reproducer.)

## Reproducer
Pure source-level / static observation: the OR at line 2520-2521 has an unreachable first alternative. No `.ll` reproducer is needed for the dead-code claim itself. The follow-on "Output == Dynamic with non-IEEE Input" misfold is currently not observable through `opt -passes=instsimplify` on a `"denormal-fp-math"="dynamic,preserve-sign"`-annotated function (the attribute plumbing does not reach `Function::getDenormalMode` in the tested 23.0.0git build), so the failure mode is structural rather than directly externally observable today. A future commit that wires the attribute through `Attribute::DenormalFPEnv` properly will start exposing the gap.

## Fix sketch
1. **Minimum**: delete the dead arm at line 2520-2521 so the conditional reads simply
   ```cpp
   if (DenormMode.Input  == DenormalMode::IEEE &&
       DenormMode.Output == DenormalMode::Dynamic)
     return nullptr;
   ```
2. **Defensive**: extend the bail-out to `Output == Dynamic` unconditionally, since the folder cannot pick a sign for a flushed result when the runtime decides at run-time whether to flush at all:
   ```cpp
   if (DenormMode.Input  == DenormalMode::Dynamic ||
       DenormMode.Output == DenormalMode::Dynamic)
     return nullptr;
   ```

## Severity
Low-medium. The dead-code observation is unambiguous. The downstream "wrong sign for a flushed denormal when Output is Dynamic" gap is only reachable if the front end wires `Attribute::DenormalFPEnv` such that `Output == Dynamic` is paired with a non-IEEE `Input` — which is unusual but not impossible for HPC stacks doing runtime denormal-mode configuration.

## Confidence
High that the OR's first alternative is dead. Medium-low that the fall-through case is also a correctness concern in current shipping code — the immediate fix is the cosmetic one; the broader fix is a hardening.

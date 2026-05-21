# w413 — `constantFoldCanonicalize` dead-code in denormal-mode handler shadowed by earlier early-return

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
  if ((DenormMode.Input == DenormalMode::Dynamic &&         //  <-- DEAD: Input==Dynamic was just returned above
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

The early bail-out at line 2516-2517 (`if (DenormMode.Input == DenormalMode::Dynamic) return nullptr;`) makes the first alternative inside the OR at line 2520-2521 (`DenormMode.Input == DenormalMode::Dynamic && DenormMode.Output == DenormalMode::IEEE`) **unreachable** — by the time control reaches the OR, `DenormMode.Input` is provably not `Dynamic`.

The OR is therefore equivalent to just the second alternative:
```cpp
if (DenormMode.Input == DenormalMode::IEEE &&
    DenormMode.Output == DenormalMode::Dynamic)
  return nullptr;
```

This is at minimum a code-clarity bug (the OR is misleading about what cases are actually being guarded against). More worryingly, the surviving "active" alternative leaves a coverage gap: it only bails when `Input == IEEE && Output == Dynamic`. It does **not** bail when `Output == Dynamic` is paired with `Input == PreserveSign` or `Input == PositiveZero` — these silently fall through to the `IsPositive` computation at line 2526-2529, which derives the sign purely from the *input* denormal mode without consulting `Output` at all (other than the specific `Output == PositiveZero && Input == IEEE` arm, which we just excluded for `Output == Dynamic`).

## Why this matters
When the function's denormal-output mode is `Dynamic`, the folder fundamentally cannot know whether a denormal output would be left intact or flushed to zero by the runtime. Folding ahead of time picks one behaviour and bakes it into the IR, removing the runtime configurability that the `Dynamic` mode is meant to preserve.

Worked case: function has `"denormal-fp-math"="preserve-sign,dynamic"` (Input=PreserveSign, Output=Dynamic). The current code:
1. line 2513: `DenormMode == IEEE` → false, skip.
2. line 2516: `Input == Dynamic` → false (Input=PreserveSign), skip.
3. line 2520-2524: `Input == Dynamic && Output == IEEE` → false; `Input == IEEE && Output == Dynamic` → false (Input=PreserveSign). Whole OR is false, skip.
4. line 2526-2529: `IsPositive = !Src.isNegative() || Input == PositiveZero || (Output == PositiveZero && Input == IEEE)`. With Input=PreserveSign, both special-case arms are false, so `IsPositive = !Src.isNegative()`.
5. line 2531-2532: return a zero with sign preserved.

So the folder commits to returning `±0` for a denormal input even though the runtime denormal-output mode is `Dynamic`. If the runtime mode happens to be IEEE (denormals preserved), this is a miscompile — the runtime would have produced the denormal back unchanged, but the folder forced a flush.

## Reproducer
Demonstrating this requires constructing a function with mixed denormal modes. The simplest is via the `"denormal-fp-math"` function attribute:

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"

declare double @llvm.canonicalize.f64(double)

define double @t() #0 {
  ; smallest positive double denormal: 0x0000000000000001
  %r = call double @llvm.canonicalize.f64(double 0x0000000000000001)
  ret double %r
}

attributes #0 = { "denormal-fp-math"="preserve-sign,dynamic" }
```

`opt -passes=instsimplify -S` folds this to `ret double 0.000000e+00` (zero with sign preserved). Per the function attribute, the runtime output mode is dynamic and may or may not flush — but the folder has committed to "flush".

## Fix sketch
1. **At minimum**: delete the dead arm of the OR at line 2520-2521 (cosmetic; clarifies intent).
2. **Correct**: extend the bail-out to *also* return `nullptr` whenever `DenormMode.Output == DenormalMode::Dynamic` (regardless of `Input`), because the folder cannot speak for the runtime's choice of how to materialise a denormal output. Concretely:

```cpp
if (DenormMode.Input  == DenormalMode::Dynamic ||
    DenormMode.Output == DenormalMode::Dynamic)
  return nullptr;
```

## Severity
Moderate. Reachable only on inputs that:
1. are denormal,
2. live in a function whose `"denormal-fp-math"` attribute pairs a non-IEEE input mode with a `Dynamic` output mode,
3. call `llvm.canonicalize`.

This is uncommon in user code but not impossible — front ends that use dynamic denormal flushing (some HPC stacks, some GPU targets) configure exactly this. The dead-code observation is unambiguous and the coverage gap is real.

## Confidence
High that the dead-code observation is correct: the early return at line 2516-2517 unconditionally provably-eliminates the first arm of the OR at line 2520-2521. Medium that the "Output == Dynamic with non-IEEE Input" silent fold is treated by LLVM as a bug rather than as deliberate (the existing logic does try to cover several `Output == PositiveZero` paths, so the intent seems to be conservative bail-out on `Dynamic` outputs — the cited bail-out simply wasn't updated to cover that case).

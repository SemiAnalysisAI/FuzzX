# w275: optimizeFMod sets `nnan` on frem when input is known NaN

**Severity:** Miscompile.

**Where:** `llvm/lib/Transforms/Utils/SimplifyLibCalls.cpp:2854-2880`
(file path: `/home/orenamd@semianalysis.com/FuzzX/amdgpu/third_party/llvm-project/llvm/lib/Transforms/Utils/SimplifyLibCalls.cpp`)

## Root cause

`optimizeFMod` is structured to detect whether a libc `fmod(x, y)` call could set
errno. errno is set only when `x` is `+/-inf` or `y == 0`. The variable holding
the proof-of-no-errno is unfortunately named `IsNoNan`:

```cpp
2859:  bool IsNoNan = CI->hasNoNaNs();
2860:  if (!IsNoNan) {
2861:    SimplifyQuery SQ(DL, TLI, DT, AC, CI, true, true, DC);
2862:    KnownFPClass Known0 = computeKnownFPClass(CI->getOperand(0), fcInf, SQ);
2863:    if (Known0.isKnownNeverInfinity()) {
2864:      KnownFPClass Known1 =
2865:          computeKnownFPClass(CI->getOperand(1), fcZero | fcSubnormal, SQ);
2866:      ...
2869:      IsNoNan = Known1.isKnownNeverLogicalZero(F->getDenormalMode(FltSem));
2870:    }
2871:  }
2872:
2873:  if (IsNoNan) {
2874:    Value *FRem = B.CreateFRemFMF(CI->getOperand(0), CI->getOperand(1), CI);
2875:    if (auto *FRemI = dyn_cast<Instruction>(FRem))
2876:      FRemI->setHasNoNaNs(true);   // <-- BUG
2877:    return FRem;
2878:  }
```

The pass only proves "x is not `Inf`" and "y is not `0`" — it does **not** prove
that the inputs are non-NaN. But then on line 2876 it unconditionally attaches
the `nnan` FMF flag to the replacement `frem`.

If `x` is NaN (and not infinity), the original `fmod(x, 1.0)` is well-defined and
returns NaN per the C standard (no errno change). After the transform, the
replacement is `frem nnan double NaN, 1.0`, which produces a NaN value;
because `nnan` promises the result is non-NaN, the value is **poison**, and any
downstream consumer that does math/`fcmp`/`select` on it becomes
miscompilation fuel.

The fix is to only set `nnan` on the new `frem` when the source `CallInst`
itself had `nnan` (or when both operands are provably non-NaN). The condition
`Known0.isKnownNeverInfinity() && Known1.isKnownNeverLogicalZero(...)` proves
*no errno*, **not** *no NaN*. The renaming `IsNoNan` → `IsNoErrno` would
make this obvious.

## Reproducer

```ll
; opt -passes=instcombine -S
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare double @fmod(double, double)
declare void @llvm.assume(i1)

define i1 @test(double %x) {
  %isnan = fcmp uno double %x, 0.0
  call void @llvm.assume(i1 %isnan)
  %r = call double @fmod(double %x, double 1.0)
  ; isnan(r): r != r
  %check = fcmp uno double %r, %r
  ret i1 %check
}
```

**Source semantics:** `%x` is asserted NaN. `fmod(NaN, 1.0) = NaN` (C99
F.10.7.1). `isnan(NaN) = true`. Function must return `i1 true`.

**After `opt -O2 -S`:**

```ll
define noundef i1 @test(double %x) local_unnamed_addr {
  ret i1 false
}
```

`opt -passes='instcombine<no-verify-fixpoint>' -S` shows the bad rewrite directly:

```ll
%r = frem nnan double %x, 1.000000e+00
```

(after assume‑driven simplification has already replaced the unordered fcmp with
its propagation result). The `nnan` flag is the carrier of the miscompile —
GVN / SimplifyCFG then poisons the `fcmp uno` and folds it to `false`.

## Suggested fix

```cpp
  if (IsNoErrno) {  // renamed
    Value *FRem = B.CreateFRemFMF(CI->getOperand(0), CI->getOperand(1), CI);
    // Do NOT add nnan unconditionally; the original FMF (copied via
    // CreateFRemFMF) already carries the correct nnan bit from CI.
    return FRem;
  }
```

Removing the explicit `setHasNoNaNs(true)` (line 2876) is sufficient: the FMF
on the source `CallInst` is already propagated through `CreateFRemFMF`.

## Default x86 -O2 only

Reproduces with `opt -O2 -S` on `x86_64-unknown-linux-gnu`; no source-level
changes required.

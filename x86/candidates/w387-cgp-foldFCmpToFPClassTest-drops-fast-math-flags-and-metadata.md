# w387: CodeGenPrepare::foldFCmpToFPClassTest drops fast-math flags and metadata

**File:** `llvm/lib/CodeGen/CodeGenPrepare.cpp`
**Lines:** 2064-2095 (`foldFCmpToFPClassTest`), with the offending rewrite at 2090-2094
**Function:** `foldFCmpToFPClassTest` (called from `CodeGenPrepare::optimizeCmp`, line 2289)
**Severity:** Information loss — fast-math flags (`nnan`, `ninf`, etc.) and any other instruction-level metadata attached to the original `fcmp` are silently dropped when it is rewritten into `@llvm.is.fpclass`.

## Summary

`foldFCmpToFPClassTest` recognizes patterns like `fcmp ueq x, +Inf` and rewrites them into `is.fpclass(x, fcInf|fcNan)` (because the canonicalization of `fcmp` away from `is.fpclass` should be reversed when the result will not be folded into FABS on this target).

The rewrite is:

```c++
IRBuilder<> Builder(Cmp);
Value *IsFPClass = Builder.createIsFPClass(ClassVal, ClassTest);
Cmp->replaceAllUsesWith(IsFPClass);
RecursivelyDeleteTriviallyDeadInstructions(Cmp);
```

When the original `Cmp` has fast-math flags such as `nnan` or `ninf`, those flags are valid analysis facts that downstream consumers (e.g. SDAG combines, branch optimizations, profile/PGO mappers) can use. After rewrite, the new intrinsic call has no FMF and no metadata, and downstream code can no longer take advantage of the flags.

Note: `is.fpclass` returns `i1`, so currently the IR does not let us hang FMF directly on the call. However, the FCmp's class-bits should be **narrowed** based on the FCmp's FMF before constructing the intrinsic — e.g. `nnan` means we know the input is not NaN, so `ClassTest = fcInf|fcNan` should become `ClassTest = fcInf` (a strictly smaller class), which yields strictly better codegen downstream. The current implementation passes the FMF-naive `ClassTest` unchanged.

## Source

```c++
// llvm/lib/CodeGen/CodeGenPrepare.cpp:2064-2094
static bool foldFCmpToFPClassTest(CmpInst *Cmp, const TargetLowering &TLI,
                                  const DataLayout &DL) {
  FCmpInst *FCmp = dyn_cast<FCmpInst>(Cmp);
  if (!FCmp)
    return false;
  ...
  auto [ClassVal, ClassTest] =
      fcmpToClassTest(FCmp->getPredicate(), *FCmp->getParent()->getParent(),
                      FCmp->getOperand(0), FCmp->getOperand(1));
  if (!ClassVal)
    return false;

  if (!ShouldReverseTransform(ClassTest) && !ShouldReverseTransform(~ClassTest))
    return false;

  IRBuilder<> Builder(Cmp);
  Value *IsFPClass = Builder.createIsFPClass(ClassVal, ClassTest);
  Cmp->replaceAllUsesWith(IsFPClass);
  RecursivelyDeleteTriviallyDeadInstructions(Cmp);
  return true;
}
```

No FMF inspection (`FCmp->hasNoNaNs()` etc.), no `copyMetadata(*FCmp, ...)`.

## Reproducer (`test_fcmp_class.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"

declare double @llvm.fabs.f64(double)

define i1 @test(double %x) {
entry:
  ; fcmp ueq with +Inf canonicalizes to ClassTest = fcInf|fcNan.
  ; Adding `nnan` on the fcmp asserts the input is not NaN, so the
  ; class test should narrow to just fcInf — but the FMF is silently dropped.
  %abs = call double @llvm.fabs.f64(double %x)
  %cmp = fcmp nnan ninf nsz arcp contract afn reassoc ueq double %abs, 0x7FF0000000000000
  ret i1 %cmp
}
```

## Reproduce

```
$ /home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc \
    -mtriple=x86_64-unknown-linux-gnu -O2 -stop-after=codegenprepare \
    test_fcmp_class.ll -o -
```

## Observed IR after CGP

```llvm
define i1 @test(double %x) {
entry:
  %0 = call i1 @llvm.is.fpclass.f64(double %x, i32 519)
  ret i1 %0
}
```

Where `519 = 0x207 = fcSubnormal? + fcZero? ...` — actually `519 = fcInf | fcNan` (fcPosInf=0x200, fcNegInf=0x004, fcSNan=0x001, fcQNan=0x002, sum 0x207=519).

The class bits include `fcNan` even though the original fcmp had `nnan`. With `nnan`, the new intrinsic should have been `call i1 @llvm.is.fpclass.f64(double %x, i32 516)` (`fcInf` only), which has materially better X86 lowering (just a magnitude compare, no NaN special-case).

## Suggested fix

Inspect the FCmp's FMF before constructing the intrinsic and narrow the class test:

```c++
auto [ClassVal, ClassTest] =
    fcmpToClassTest(FCmp->getPredicate(), *FCmp->getParent()->getParent(),
                    FCmp->getOperand(0), FCmp->getOperand(1));
if (!ClassVal)
  return false;

FPClassTest NarrowedTest = ClassTest;
if (FCmp->hasNoNaNs())
  NarrowedTest &= ~fcNan;
if (FCmp->hasNoInfs())
  NarrowedTest &= ~fcInf;
...

Value *IsFPClass = Builder.createIsFPClass(ClassVal, NarrowedTest);
```

(Plus optionally propagate any non-debug metadata via `copyMetadata`.)

## Impact

- Lost optimization opportunity: the X86 lowering of `is.fpclass(x, fcInf|fcNan)` is materially worse than `is.fpclass(x, fcInf)` (the former needs to handle NaN, the latter does not). Equivalent FMF-driven narrowings apply for `ninf`, `nsz`, etc.
- Programmer-asserted FMF facts (which the IR Producer explicitly added to the fcmp) are not honored by CGP's rewrite.
- The pattern matches the recently-filed w205..w209 "CGP IR-flag loss" series.

# InstCombine foldFPtoI uses fcPosNormal for fptoui, fcNormal for fptosi

File: `llvm/lib/Transforms/InstCombine/InstCombineCasts.cpp` lines 2408-2418

```cpp
static Instruction *foldFPtoI(Instruction &FI, InstCombiner &IC) {
  // fpto{u/s}i non-norm --> 0
  FPClassTest Mask =
      FI.getOpcode() == Instruction::FPToUI ? fcPosNormal : fcNormal;
  KnownFPClass FPClass = computeKnownFPClass(
      FI.getOperand(0), Mask, IC.getSimplifyQuery().getWithInstruction(&FI));
  if (FPClass.isKnownNever(Mask))
    return IC.replaceInstUsesWith(FI, ConstantInt::getNullValue(FI.getType()));
  return nullptr;
}
```

## Reasoning

For `FPToUI`, the mask is `fcPosNormal` (positive normals only). Folding
to 0 when the value is known to never be a positive normal claims:
"all denormals, zeros, +/-infinities, NaN, and negative normals produce 0
under fptoui". That's true under LangRef *only* because:
  - +/-denormal → in [0, smallest_normal): truncates to 0 ✓
  - +/-0 → 0 ✓
  - +inf, -inf, NaN → poison (any value is "OK", including 0) ✓
  - negative normals → poison under fptoui (out of unsigned range) ✓

So the fold is technically poison-refining for the inf/NaN/negative
cases, which is allowed.

However, the asymmetry with `FPToSI` is suspicious. For `FPToSI` the
mask is `fcNormal` (all normals, positive and negative). Folding to 0
when the value is "never a normal" claims:
  - denormal → in (-1, 1): truncates to 0 ✓
  - +/-0 → 0 ✓
  - inf, NaN → poison ✓

This is correct for `FPToSI` because both signs of denormal truncate to
0. But the asymmetric mask choice raises the question: why is the
FPToUI mask `fcPosNormal` rather than `fcNormal`? With `fcPosNormal`,
a value that is known to be a *negative normal* would be folded to 0,
which is poison-refinement (negative normals are out-of-range for
fptoui and thus poison). That's allowed but it means InstCombine is
silently converting a poison-producing fptoui into a defined zero.
For a sanitizer build (`-fsanitize=float-cast-overflow`) this fold
eliminates UB reports.

## Repro

```
; opt -passes=instcombine -S
define i32 @f(double %x) {
  %abs = call double @llvm.fabs.f64(double %x)
  %neg = fsub double -0.0, %abs    ; %neg is known <= 0, so never a positive normal once non-zero
  %r = fptoui double %neg to i32   ; poison for any %neg < 0
  ret i32 %r
}
declare double @llvm.fabs.f64(double)
```

## Expected wrong outcome

For a sanitizer-instrumented build, the `fptoui` of a negative value
would be UB; this fold turns it into `ret i32 0`, hiding the bug from
the sanitizer. Not a wrong-code bug in the LangRef sense (poison
refinement is permitted), but a sanitizer-defeating behavior on x86
targets where the runtime trap would have been reachable.

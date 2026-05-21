# w242: uitofp/fptoui chain via narrow integer folds to trunc — soundness via UB

## File / Region
- `llvm/lib/Transforms/InstCombine/InstCombineCasts.cpp`,
  `InstCombinerImpl::foldItoFPtoI`, lines ~2340-2400.

## Code
```cpp
if (!isKnownExactCastIntToFP(*OpI)) {
  if constexpr (!IsSaturating) {
    // The first cast may not round exactly based on the source integer width
    // and FP width, but the overflow UB rules can still allow this to fold.
    // If the destination type is narrow, that means the intermediate FP value
    // must be large enough to hold the source value exactly.
    //
    // For example, (uint8_t)((float)(uint32_t 16777217) is UB.
    int OutputSize = (int)DestType->getScalarSizeInBits();
    if (OutputSize > OpI->getType()->getFPMantissaWidth())
      return nullptr;
  } else {
    // Sat intrinsics produce a defined saturated value on overflow, so
    // the UB-based shortcut is invalid. Require exactness.
    return nullptr;
  }
}
```

Followed by:
```cpp
if (DestWidth < SrcWidth)
  return new TruncInst(X, DestType);
```

## Observation
`(uint8_t)((float)x)` where `x` is `i32` folds to `trunc i32 x to i8`.
The fold relies on the fact that any `uint32_t > 255` cast to float and then
to `uint8_t` overflows the destination type, which is **undefined behavior**
in LLVM (poison).

## Analysis (Alive2-style)
For X in [0, 255]: `uitofp X to float` = exactly X.0 (since X < 2^24). 
`fptoui X.0 to i8` = X (exact). 
`trunc X` = X (since X fits in i8). **Match.**

For X in [256, 2^24]: `uitofp X to float` = X.0 (exact). 
`fptoui X.0 to i8` is **poison** (overflow). 
`trunc X` returns X mod 256 (some defined value). 
This is **refinement of poison to a defined value** — sound in LLVM.

For X in [2^24+1, UINT_MAX]: `uitofp X to float` may round. 
`fptoui (rounded X) to i8` is poison (overflow). 
`trunc X` returns X mod 256. Refinement, sound.

The fold correctly excludes the saturating variants (lines 2375-2378)
because saturating intrinsics produce defined values on overflow.

## Reproducer
Source: `/tmp/w240/t25_int_fp_int.ll`

```llvm
define i8 @itof_ftoi_narrowing(i32 %x) {
  %f = uitofp i32 %x to float
  %r = fptoui float %f to i8
  ret i8 %r
}
```
folds to:
```llvm
define i8 @itof_ftoi_narrowing(i32 %x) {
  %r = trunc i32 %x to i8
  ret i8 %r
}
```

## Verdict
**NOT a miscompile.** The fold is correct, relying on UB semantics for
overflow in the non-saturating `fptoui`. The saturating variants are
correctly excluded.

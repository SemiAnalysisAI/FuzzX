# w04: simplifyX86pmulh "multiply by one" m_One() matches vector with undef lanes, fold loses info

File: llvm/lib/Target/X86/X86InstCombineIntrinsic.cpp, lines 519–526

## Reasoning

```cpp
if (!IsRounding) {
  if (match(Arg0, m_One()))
    return IsSigned ? Builder.CreateAShr(Arg1, 15)
                    : ConstantAggregateZero::get(ResTy);
  if (match(Arg1, m_One()))
    return IsSigned ? Builder.CreateAShr(Arg0, 15)
                    : ConstantAggregateZero::get(ResTy);
}
```

`m_One()` in PatternMatch.h is documented as: "Match an integer 1 or a vector with all elements
equal to 1. **For vectors, this includes constants with undefined elements.**"

So `Arg0 = <i16 1, i16 undef, i16 1, ..., i16 1>` will match. The rewrite then becomes
`AShr(Arg1, 15)` for the signed PMULHW case and `0` for the unsigned PMULHUW case. That is a
refinement (undef -> defined value), which the LLVM IR rules permit, so this is not necessarily
wrong on its own.

The hazard is the **unsigned** case: with `Arg0 = <i16 1, i16 undef, ...>`, the rewrite returns
the all-zero vector. That hides the fact that for the undef lane, `Arg1` could still propagate
information into a poison-producing use downstream (e.g., a later `div` by that lane). The fold
drops the data dependency on `Arg1` entirely when Arg0 has a mix of 1s and undefs. A reasonable
expectation when an undef is in Arg0 is "the lane result is some i16 selected from
zext(Arg1[lane]) * zext(undef) >> 16", which depends on Arg1.

A cleaner reading: when the spec says PMULHUW(undef, x) = some i16, the fold returning 0
unconditionally is a stronger refinement than necessary and can confuse downstream UB analysis.
For the signed case, returning `AShr(Arg1, 15)` for undef Arg0 lanes is reasonable, because the
result is one of {-1, 0} for any Arg1 value, and AShr(undef, 15) is a valid refinement.

This is borderline but worth a candidate because the surrounding code (the very first guard) is:

```cpp
if (isa<UndefValue>(Arg0) || isa<UndefValue>(Arg1))
  return ConstantAggregateZero::get(ResTy);
```

which checks **full** undef, not partial. The `m_One()` path then quietly handles partial undef
in a way the author may not have intended.

## Concrete IR

```llvm
declare <8 x i16> @llvm.x86.sse2.pmulhu.w(<8 x i16>, <8 x i16>)

define <8 x i16> @pmulhu_one_with_undef(<8 x i16> %x) {
  %r = call <8 x i16> @llvm.x86.sse2.pmulhu.w(
        <8 x i16> <i16 1, i16 undef, i16 1, i16 1, i16 1, i16 1, i16 1, i16 1>,
        <8 x i16> %x)
  ret <8 x i16> %r
}
```

Expected fold (current): `<8 x i16> zeroinitializer`.

The undef lane in the splat-of-one means hardware-defined behavior is "high 16 of (anything as
u16) * x[1] = 0 if x[1] fits in 16 bits unsigned, which it always does, so still 0." So the
result happens to coincide. But the abstract semantics being more permissive than the fold is
a fingerprint of a class of bugs to watch for in similar `m_One()` / `m_Zero()` matches in this
file (e.g. for PMADDWD / PMULDQ if those ever grow such folds).

## Expected wrong result

Construct a case where downstream UB depends on the data flow from Arg1. E.g.:

```llvm
define i32 @poison_leak(<8 x i16> %x) {
  %r = call <8 x i16> @llvm.x86.sse2.pmulhu.w(
        <8 x i16> <i16 1, i16 undef, i16 1, i16 1, i16 1, i16 1, i16 1, i16 1>,
        <8 x i16> %x)
  %e = extractelement <8 x i16> %r, i32 1
  %z = zext i16 %e to i32
  %div = udiv i32 100, %z              ; UB if %z == 0
  ret i32 %div
}
```

Before InstCombine: `%z` is the high 16 bits of `1 * x[1]` (unsigned) = 0, so `udiv 100, 0` is
always UB — the program is undefined. After InstCombine: `%r` = zero vector, `%z` = 0, `udiv` is
UB. Same UB. So in this specific instance, equivalent.

The hazard is in PMULH(udf-elt, x) **signed** path returning `AShr(x, 15)` — for any `x[1]` value
the result is in {-1, 0}, but the original signed product `sext(undef) * sext(x[1])` could be
anything in [-2^30, 2^30], with high bits arbitrary. The fold restricts to {-1, 0}, missing valid
results, but that's a refinement, so fine.

Net: this is a **non-bug** under strict LLVM semantics but is worth flagging as fragile pattern
to drop priority on.

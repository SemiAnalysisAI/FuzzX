# w211 — `ldexp(ldexp(x, a), b)` fold ignores integer overflow of `a + b`

## Where
`llvm/lib/Transforms/InstCombine/InstCombineCalls.cpp:3232-3247`

```cpp
if (match(Src, m_OneUse(m_Intrinsic<Intrinsic::ldexp>(
                   m_Value(InnerSrc), m_Value(InnerExp)))) &&
    Exp->getType() == InnerExp->getType()) {
  FastMathFlags FMF = II->getFastMathFlags();
  FastMathFlags InnerFlags = cast<FPMathOperator>(Src)->getFastMathFlags();

  if ((FMF.allowReassoc() && InnerFlags.allowReassoc()) ||
      signBitMustBeTheSame(Exp, InnerExp, SQ.getWithInstruction(II))) {
    // TODO: Add nsw/nuw probably safe if integer type exceeds exponent
    // width.
    Value *NewExp = Builder.CreateAdd(InnerExp, Exp);
    II->setArgOperand(1, NewExp);
    II->setFastMathFlags(InnerFlags); // Or the inner flags.
    return replaceOperand(*II, 0, InnerSrc);
  }
}
```

## What's wrong

The fold combines `ldexp(ldexp(x, a), b)` into `ldexp(x, a + b)` whenever:
1. `reassoc` is set on both calls, **or**
2. `signBitMustBeTheSame(a, b)` — both known non-negative OR both known non-positive.

The comment justifying path (2) says: *"safe to fold if we know both exponents are >= 0 or <= 0 since it would just double down on the overflow/underflow which would occur anyway."*

**That justification is wrong.** When both exponents are non-negative and large, the integer `add` can **wrap around to a negative value** in the signed i32 exponent type used by `llvm.ldexp.*`. The folded `ldexp(x, wrapped_negative)` then computes `x * 2^negative` — i.e., the opposite of the intended overflow.

The TODO comment on line 3241 (*"Add nsw/nuw probably safe if integer type exceeds exponent width"*) acknowledges this, but the fold still fires without any check that the constants used for `signBitMustBeTheSame` rule out overflow.

## Severity / class
**Definite miscompile.** Numerically wrong result. Triggers with any program that uses chained `ldexp` (e.g., scientific code packing power-of-two scaling, half-float emulation libraries, `std::frexp`/`ldexp` round-trips) and lets the compiler prove via known bits that both exponents have the same sign.

## Reproducer

```ll
; opt -passes=instcombine -S
define double @repro(double %x, i32 %e1, i32 %e2) {
  ; Both exponents are forced non-negative via clearing the sign bit.
  ; This is enough for signBitMustBeTheSame() to succeed.
  %a1 = and i32 %e1, 2147483647    ; clear sign bit, so a1 >= 0
  %a2 = and i32 %e2, 2147483647    ; clear sign bit, so a2 >= 0
  %r1 = call double @llvm.ldexp.f64.i32(double %x, i32 %a1)
  %r2 = call double @llvm.ldexp.f64.i32(double %r1, i32 %a2)
  ret double %r2
}

declare double @llvm.ldexp.f64.i32(double, i32)
```

After `opt -passes=instcombine -S`:

```ll
define double @repro(double %x, i32 %e1, i32 %e2) {
  %a1 = and i32 %e1, 2147483647
  %a2 = and i32 %e2, 2147483647
  %1 = add nuw i32 %a1, %a2
  %r2 = call double @llvm.ldexp.f64.i32(double %x, i32 %1)
  ret double %r2
}
```

The fold produced `add nuw` (proven non-wrapping in **unsigned** arithmetic) but the exponent is interpreted as **signed**. For `%e1 = %e2 = INT_MAX (0x7FFFFFFF)`:
- Original: `ldexp(ldexp(x, INT_MAX), INT_MAX) = ldexp(+inf, INT_MAX) = +inf` (for finite non-zero positive x).
- Folded: `add(INT_MAX, INT_MAX) = 0xFFFFFFFE` reinterpreted as signed i32 = `-2`. `ldexp(x, -2) = x / 4`.

For `x = 1.0`: original = `+inf`, folded = `0.25`.

### Even cleaner: all constants

```ll
define double @repro2(double %x) {
  %a1 = and i32 2147483647, 2147483647
  %a2 = and i32 2147483647, 2147483647
  %r1 = call double @llvm.ldexp.f64.i32(double %x, i32 %a1)
  %r2 = call double @llvm.ldexp.f64.i32(double %r1, i32 %a2)
  ret double %r2
}
```

After `opt -passes=instcombine -S`:
```ll
define double @repro2(double %x) {
  %r2 = fmul double %x, 2.500000e-01
  ret double %r2
}
```

For `x = 1.0`, the original returns `+inf`; the folded code returns `0.25`.

### Symmetric case (both exponents negative)

```ll
define double @repro3(double %x, i32 %e1, i32 %e2) {
  %a1 = or i32 %e1, -2147483648    ; set sign bit, so a1 <= -1
  %a2 = or i32 %e2, -2147483648
  %r1 = call double @llvm.ldexp.f64.i32(double %x, i32 %a1)
  %r2 = call double @llvm.ldexp.f64.i32(double %r1, i32 %a2)
  ret double %r2
}
```

Same fold fires; `(INT_MIN) + (INT_MIN) = 0` (overflow), so the fold rewrites the chain to `ldexp(x, 0) = x` when the original underflows to zero.

## Suggested fix

In path (2), require that the `add` cannot overflow in **signed** arithmetic (i.e., the sum is provably representable as a signed i32). Either:
- Use `add nsw` and bail unless both operands have a known leading bit (i.e., `KnownBits` of each shows the high bit is clear/set with the other operand bounded), or
- Tighten `signBitMustBeTheSame` to additionally require `nsw`-safe addition (e.g., one operand's `KnownBits.countMinLeadingZeros()` is large enough), or
- Apply the TODO suggestion: widen the add to a larger integer type, compute, then `trunc`/saturate.

Alternatively, leave the fold restricted to `reassoc`-only (where the IR contract explicitly allows the optimizer to change overflow/underflow semantics).

## Notes

- The same bug shape was theorized in the comment on line 3241; this candidate confirms it triggers in real IR.
- Trigger requires constant or `KnownBits`-derived high-bit info on both exponents — common in scaling code that masks the exponent before passing it in.
- Confirmed on `opt --version`: LLVM 23.0.0git (Default target x86_64-unknown-linux-gnu).

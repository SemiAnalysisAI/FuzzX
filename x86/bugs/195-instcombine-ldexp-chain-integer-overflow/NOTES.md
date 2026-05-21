# 195 — InstCombine `ldexp(ldexp(x, a), b)` fold silently wraps the i32 exponent sum, inverting overflow

Component: InstCombine (default O2)

## Source

`llvm/lib/Transforms/InstCombine/InstCombineCalls.cpp` lines ~3232-3247 — the `Intrinsic::ldexp` chained-call fold. The fold rewrites `ldexp(ldexp(x, a), b)` to `ldexp(x, a + b)` whenever `signBitMustBeTheSame(a, b)` holds. The justification comment claims this is safe ("double down on the overflow/underflow"). It is NOT safe: the i32 addition can wrap so a + b ends up the **opposite** sign of a and b, producing the *opposite* of the intended overflow direction. A TODO at line 3241 foreshadows the issue ("Add nsw/nuw probably safe if integer type exceeds exponent width") but no guard is implemented.

## Reproducer

```ll
define double @repro(double %x) {
  %a1 = and i32 2147483647, 2147483647   ; INT_MAX, non-negative
  %a2 = and i32 2147483647, 2147483647
  %r1 = call double @llvm.ldexp.f64.i32(double %x, i32 %a1)
  %r2 = call double @llvm.ldexp.f64.i32(double %r1, i32 %a2)
  ret double %r2
}
```

`opt -passes=instcombine -S` → `%r2 = fmul double %x, 2.500000e-01`.

i32 `INT_MAX + INT_MAX = -2`. `ldexp(x, -2)` = `x * 0.25`.

For `x = 1.0`: mathematically `ldexp(ldexp(1.0, INT_MAX), INT_MAX)` should saturate to `+inf`. The fold produces `0.25`. **Off by an infinity.**

Symmetric: `ldexp(ldexp(x, INT_MIN), INT_MIN)` should be `0` (underflow). i32 `INT_MIN + INT_MIN = 0`. The fold gives `ldexp(x, 0) = x`.

## Severity

Real Alive2-falsifiable miscompile in default `-O2`. Fires for any chained `ldexp` where both exponents have known-equal sign bit. Visible in emitted machine code: instead of a libm call to `ldexp`, llc emits a single `mulsd xmm0, 0.25`.

## Fix sketch

Either:
1. Require both `a` and `b` to be small (e.g., `getActiveBits() < bitwidth - 1` for both), so the sum can't wrap.
2. Use a wider-than-i32 addition for the combined exponent (and clamp).
3. Emit `select(overflow, sign(a) ? +inf : 0, ldexp(x, a+b))` with explicit saturation.

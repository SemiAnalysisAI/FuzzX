# w414 — `llvm.ldexp` with i64 exponent silently truncates to `int`, yielding a wildly wrong result (miscompile)

## Component
`llvm/lib/Analysis/ConstantFolding.cpp` — `Intrinsic::ldexp` handler in `ConstantFoldIntrinsicCall2`

## Source citation
`llvm/lib/Analysis/ConstantFolding.cpp:3715-3719`:

```cpp
case Intrinsic::ldexp: {
  return ConstantFP::get(
      Ty->getContext(),
      scalbn(Op1V, Op2C->getSExtValue(), APFloat::rmNearestTiesToEven));
}
```

`scalbn` (declared at `llvm/include/llvm/ADT/APFloat.h:1640`):
```cpp
inline APFloat scalbn(APFloat X, int Exp, APFloat::roundingMode RM) {
```

`Op2C->getSExtValue()` returns `int64_t`; the call site silently *narrows* it to `int` (32-bit on every x86 host LLVM supports). Any wide-i64 exponent value outside `[INT_MIN, INT_MAX]` is silently truncated mod 2^32, producing a `scalbn` argument that has no relationship to the user's intended exponent.

## Background on the intrinsic
`llvm.ldexp` is polymorphic in both the floating type and the integer exponent type:
```td
def int_ldexp : DefaultAttrsIntrinsic<[llvm_anyfloat_ty],
                                      [LLVMMatchType<0>, llvm_anyint_ty]>;
```
(`llvm/include/llvm/IR/Intrinsics.td`). So `llvm.ldexp.f64.i64` is a legal mangling. LangRef:

> "If the result overflows, the result is an infinity with the same sign."

So `ldexp(1.0, 4294967330)` must fold to `+inf`. The folder produces `2^34` instead, because the i64 exponent is silently masked to its low 32 bits (`4294967330 & 0xFFFFFFFF == 34`).

## Reproducer (`/tmp/cf_hunt/ldexp_i64.ll`)
```llvm
declare double @llvm.ldexp.f64.i64(double, i64)

; 4294967330 == 2^32 + 34. Truncated to int = 34.
define double @t1() {
  %r = call double @llvm.ldexp.f64.i64(double 1.0, i64 4294967330)
  ret double %r
}

; -4294967330 mod 2^32 = 34, but signed-narrowed -> -34
define double @t2() {
  %r = call double @llvm.ldexp.f64.i64(double 1.0, i64 -4294967330)
  ret double %r
}
```

Command:
```
opt -passes=instsimplify -S /tmp/cf_hunt/ldexp_i64.ll
```

## Actual output
```llvm
define double @t1() {
  ret double f0x4210000000000000      ; = 2^34   (WRONG)
}

define double @t2() {
  ret double f0x3DD0000000000000      ; = 2^-34  (WRONG)
}
```

Decoding:
- `0x4210000000000000` is IEEE-double `sign=0, exp=1057-1023=34, mantissa=0` → `2^34 = 1.71798692e10`.
- `0x3DD0000000000000` is `sign=0, exp=989-1023=-34, mantissa=0` → `2^-34 ≈ 5.82e-11`.

Both are the result of computing `scalbn(1.0, 34)` and `scalbn(1.0, -34)` respectively — i.e. the folder narrowed the i64 exponent argument to its low 32 bits.

## Expected output
Per LangRef:
```llvm
define double @t1() {
  ret double 0x7FF0000000000000      ; +inf  (result overflow)
}

define double @t2() {
  ret double 0.000000e+00            ; +0.0  (result underflow; "zero with the same sign")
}
```

## Why this matters
`ldexp(x, n)` is the standard primitive for IEEE-754 scaling; it appears in numerical kernels, FP normalisation passes, and as the lowering target for higher-level operations (e.g. `2^n` evaluation, denormal manipulation). A user who writes `ldexp(x, very_large_i64)` expecting saturating IEEE semantics will instead get an arbitrary low-32-bit-aliased value, with no diagnostic.

Worst case: a guard like `if (ldexp(x, n) > 1e300)` that should fire for a huge `n` (because the true result is `+inf > 1e300 = true`) instead silently sees a finite value and skips the guard. This is a textbook "silent wrong-result" miscompile when the source program uses i64 exponents.

## Fix sketch
Saturate-narrow before passing to `scalbn`, or call an APFloat overload of `scalbn` that takes a wider exponent type if one exists, or — minimally — clamp:

```cpp
case Intrinsic::ldexp: {
  int64_t Exp64 = Op2C->getSExtValue();
  int Exp = (Exp64 > INT_MAX) ? INT_MAX
          : (Exp64 < INT_MIN) ? INT_MIN
          : static_cast<int>(Exp64);
  return ConstantFP::get(
      Ty->getContext(),
      scalbn(Op1V, Exp, APFloat::rmNearestTiesToEven));
}
```

`scalbn(x, INT_MAX)` correctly produces `+inf` for finite non-zero x, and `scalbn(x, INT_MIN)` correctly produces `±0` — so saturating is both safe and matches the spec.

## Severity
High. The folder produces a *concrete finite value* that has no mathematical relationship to the intrinsic's semantics for any i64 exponent outside `[INT_MIN, INT_MAX]`. The IR transformations downstream of `instsimplify` will accept this value as ground truth.

## Confidence
High. The defect is structural (silent narrowing at a `int64_t → int` call boundary); the reproducer is minimal; the actual outputs (`2^34` and `2^-34`) are obviously not the values that `ldexp(1.0, ±4.29e9)` ought to produce per LangRef.

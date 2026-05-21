# w605 — InstCombine fmul-distribute under `reassoc` (no `nsz`) flips sign of zero

## Target
- `llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp:833-848`
- `foldFMulReassoc` distributes:
  - `(X + C1) * C  -->  (X * C) + (C * C1)`  (lines 833-840)
  - `(C1 - X) * C  -->  (C * C1) - (X * C)`  (lines 841-848)

Both transforms require only `reassoc` on the outer fmul and the inner fadd /
fsub (intersected as `FMF`). They never check `nsz`.

## Mechanism

When `X` cancels the constant in the inner add/sub so the inner result is
mathematically zero, the *sign* of that zero is determined by the sign of `C`
in the original expression: `0 * C = sign(C) * 0`. After the distributive
rewrite, the zero arises from a subtraction of two equal-magnitude opposite-sign
products `(C*C1) - (X*C) = (C*C1) - (C*C1) = +0.0` (per IEEE 754 round-to-nearest,
the sum of opposite-sign equal magnitudes is `+0.0`).

So with `C` negative and `X = ±C1`:
- Original returns `-0.0`.
- Distributed form returns `+0.0`.

This is observable through `copysign`, `signbit`, `1/x`, etc., and is precisely
what `nsz` is meant to gate. The transform is performed under `reassoc`
alone, with no `nsz` guard.

## .ll repro

```llvm
; opt -passes=instcombine -S
define double @fadd_distribute(double %x) {
  %a = fadd reassoc double %x, 3.0
  %r = fmul reassoc double %a, -2.0
  ret double %r
}

define double @fsub_distribute(double %x) {
  %s = fsub reassoc double 3.0, %x
  %r = fmul reassoc double %s, -2.0
  ret double %r
}
```

## opt diff

```
define double @fadd_distribute(double %x) {
-  %a = fadd reassoc double %x, 3.000000e+00
-  %r = fmul reassoc double %a, -2.000000e+00
+  %1 = fmul reassoc double %x, -2.000000e+00
+  %r = fadd reassoc double %1, -6.000000e+00
   ret double %r
}

define double @fsub_distribute(double %x) {
-  %s = fsub reassoc double 3.000000e+00, %x
-  %r = fmul reassoc double %s, -2.000000e+00
+  %1 = fmul reassoc double %x, -2.000000e+00
+  %r = fsub reassoc double -6.000000e+00, %1
   ret double %r
}
```

## Concrete divergence

For `%x = -3.0`:

Original `fadd_distribute`:
```
%a = fadd reassoc -3.0, 3.0    = +0.0
%r = fmul reassoc +0.0, -2.0   = -0.0
```

After instcombine:
```
%1 = fmul reassoc -3.0, -2.0   = +6.0
%r = fadd reassoc +6.0, -6.0   = +0.0   (IEEE: equal-magnitude opposite-sign -> +0)
```

Native (`cc -O0` compiled C demonstrating the same arithmetic) confirms
the two routes return `-0` and `+0` respectively (`signbit(orig)=1`,
`signbit(new)=0`).

The `fsub_distribute` case behaves identically: for `%x = +3.0` (or `-3.0` in
that pattern with sign suitably adjusted) original yields `-0.0`, transformed
yields `+0.0`.

## Discussion

Per LangRef `nsz` definition (line 4208):

> nsz - No Signed Zeros - Unless otherwise mentioned, the sign bit of 0.0 or
> -0.0 input operands can be non-deterministically flipped. This does not
> imply that -0.0 is poison and/or guaranteed to not exist in the operation.

The instcombine codebase has an existing convention (visible in issue tracker
discussion of #48998 and the patch series around D43398) that `reassoc` is
the "loosest" FMF and implicitly covers `arcp`/`afn`/`contract`-level
rewrites. Whether it also covers `nsz`-flavored sign-of-zero changes is
under-specified. The LangRef text says `reassoc` "may dramatically change
results", but does not explicitly authorize sign-of-zero changes — and the
existence of a separate `nsz` flag suggests sign-of-zero is a distinct
concern.

The existing tests in
`llvm/test/Transforms/InstCombine/fmul.ll` (`fmul_fadd_distribute`,
`fmul_fsub_distribute1`) lock in the `reassoc`-only behavior, so this is
an intended (if debatable) design choice. Either:
1. The transforms should additionally require `nsz`, or
2. The LangRef should be updated to clarify that `reassoc` subsumes sign-of-zero
   freedom (the de-facto LLVM behavior), or
3. The transforms should preserve the original sign-of-zero (much harder).

Filing this as a candidate because: (a) it produces an observably different
result without the flag (`nsz`) that LangRef seemingly reserves for that
freedom, and (b) per LangRef alone it appears unsound. If LLVM's intended
semantics is that `reassoc` implies `nsz`-style freedom, that should be
documented; if not, the transforms need an `nsz` guard.

## Severity
Low-to-medium. Reproducible value divergence (`-0.0` vs `+0.0`) under
`reassoc` without `nsz`. Observable through `copysign`, `signbit`,
division by the result (`1/-0 = -Inf`, `1/+0 = +Inf`), and any
sign-aware downstream code. Most user code that sets `reassoc` also sets
`nsz` (the `-funsafe-math-optimizations` default), so practical impact is
limited.

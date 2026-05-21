# w606 — `foldFMulReassoc` `(X / C1) * C --> X * (C / C1)` lacks `arcp` (FIXME)

## Target
- `llvm/lib/Transforms/InstCombine/InstCombineMulDivRem.cpp:814-820`
- `foldFMulReassoc` rewrites `(X / C1) * C -> X * (C / C1)` under `reassoc`
  alone. There is a `// FIXME: This seems like it should also be checking for
  arcp` comment in the source at line 815.

## Mechanism

```cpp
if (match(Op0, m_FDiv(m_Value(X), m_Constant(C1)))) {
  // FIXME: This seems like it should also be checking for arcp
  // (X / C1) * C --> X * (C / C1)
  Constant *CDivC1 =
      ConstantFoldBinaryOpOperands(Instruction::FDiv, C, C1, DL);
  if (CDivC1 && CDivC1->isNormalFP())
    return BinaryOperator::CreateFMulFMF(X, CDivC1, FMF);
  ...
}
```

This is morally `(X / C1) * C == X * (1/C1 * C) == X * (C / C1)`, i.e. the
substitution of a single multiplication for a division. That is exactly the
operation that the `arcp` ("allow reciprocal") flag is designed to gate per
LangRef:

> arcp - Allows division to be treated as a multiplication by a reciprocal.
> Specifically, this permits `a / b` to be considered equivalent to
> `a * (1.0 / b)` (which may subsequently be susceptible to code motion),
> and it also permits `a / (b / c)` to be considered equivalent to
> `a * (c / b)`.

The original computes `floor_round(X / C1)` then `floor_round(that * C)`, two
roundings on operand magnitudes. The rewrite computes `floor_round(C / C1)` at
compile time, then `floor_round(X * that)`, one rounding at runtime on possibly
different intermediate magnitude.

Because IEEE rounding does not commute through this rewrite in general, two
`fdiv`/`fmul` outputs can differ on values where the original two-rounding
chain produces a different last-bit result from the one-rounding chain.

## .ll repro (transform fires under `reassoc` alone)

```llvm
; opt -passes=instcombine -S
define float @xdiv_c1_times_c(float %x) {
  %d = fdiv reassoc float %x, 0x4045000000000000   ; 42.0
  %r = fmul reassoc float %d, 0x4051800000000000   ; 70.0
  ret float %r
}

define double @xdiv_c1_times_c_d(double %x) {
  %d = fdiv reassoc double %x, 1.000000e+01
  %r = fmul reassoc double %d, 1.000000e-01
  ret double %r
}
```

## opt diff

```
define float @xdiv_c1_times_c(float %x) {
-  %d = fdiv reassoc float %x, 4.200000e+01
-  %r = fmul reassoc float %d, 7.000000e+01
+  %r = fmul reassoc float %x, f0x3FD55555    ; ≈ 70.0/42.0
   ret float %r
}

define double @xdiv_c1_times_c_d(double %x) {
-  %d = fdiv reassoc double %x, 1.000000e+01
-  %r = fmul reassoc double %d, 1.000000e-01
+  %r = fmul reassoc double %x, 1.000000e-02
   ret double %r
}
```

Note both inputs lack the `arcp` flag; transform fires anyway.

## Discussion

The FIXME has been in the source for years. The companion routine
`foldFDivConstantDividend` (lines 1957-1989) for `C / (X * C2) --> (C / C2) / X`
and `C / (X / C2) --> (C * C2) / X` explicitly requires:

```
1969    if (!I.hasAllowReassoc() || !I.hasAllowReciprocal())
1970      return nullptr;
```

So that sister routine correctly gates the same kind of fdiv-substitution
behind both `reassoc` and `arcp`. The inconsistency between
`foldFMulReassoc` and `foldFDivConstantDividend` is the smoking gun: the
authors knew the rewrite needs `arcp` (the sister routine demands it), but
the fmul-side path was never updated.

Per the historical thread referenced in
[Issue #48998](https://github.com/llvm/llvm-project/issues/48998), there is
an undocumented LLVM convention that `reassoc` implicitly subsumes `arcp`,
`afn`, and `contract`. If that is the intended semantics, the FIXME should
just be removed and the sister routine's stricter check loosened to match.
If `arcp` is genuinely required (consistent with the sister routine), the
fix is to add an `I.hasAllowReciprocal()` guard at line 814.

Either way the current state is incoherent.

## Severity
Low. Same as w605 — without `arcp` (and `reassoc`-only inputs), the
rewrite is technically unsound per a strict LangRef reading, but in
practice users who enable `reassoc` typically also enable `arcp` via
`-funsafe-math-optimizations`. The bug is the inconsistency between the
fmul and fdiv halves of the same transform family, plus a long-standing
FIXME left in source.

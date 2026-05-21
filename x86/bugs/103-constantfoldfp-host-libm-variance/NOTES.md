# ConstantFoldLibCall2/ConstantFoldFP: host-libm-dependent transcendental folding

File: `llvm/lib/Analysis/ConstantFolding.cpp`
Functions:
  * `ConstantFoldFP` (lines 2263–2284) — single-arg host-double libm.
  * `ConstantFoldBinaryFP` (lines 2300–2310) — two-arg host-double libm.
  * `ConstantFoldLibCall2` (lines 3348–3417) — `pow`, `fmod`, `atan2`,
    `nextafter`.
  * Callers of `ConstantFoldFP` in `ConstantFoldScalarCall1` (lines
    2849–3175) for `log/log2/log10/exp/sin/cos/sinh/cosh/asin/acos/atan/
    sqrt/tan/tanh/erf/logb/log1p/atanh/...`.

## Bug pattern hit

The "FP intrinsics that use host math (can vary)" pattern from the brief.
LLVM uses the *compile host's* `libm` (`pow`, `sin`, `cos`, `log`, `exp`,
`atan2`, ...) to fold constant arguments of `llvm.sin/cos/log/exp/pow/...`.
Different libm implementations produce slightly different last-ULP results
for transcendentals (`glibc` vs. `musl` vs. Apple's `libsystem_m` vs. MSVC
`crt`). Result: the *bits* of a constant-folded `@llvm.pow.f64(c, c)` depend
on which machine LLVM was compiled-and-run on — cross-host reproducibility
broken.

This is acknowledged in part by the code (e.g. the `atan2(±0,±0)` carve-out
at line 3399 for Solaris), but only for *one* known case. The general
host-libm-precision issue is uncovered.

## Specific host-divergence corner cases

### A. `pow(-1.0, ±∞)`
C99/IEC 60559 Annex F.10.4.4: `pow(-1.0, ±∞) == 1.0`.
LLVM goes through `ConstantFoldBinaryFP(pow, ...)` (line 3377), which does:

```cpp
double Result = pow(V.convertToDouble(), W.convertToDouble());
```

Modern glibc returns 1.0; some older host libms (and certain musl versions)
return NaN. The constant folder will then bake `NaN` or `1.0` into the IR
depending on the host. Downstream code that branches on `isnan(result)` then
sees different control flow per build host.

### B. `pow(NaN, 0.0)`
C99 Annex F.10.4.4: must return 1.0 (because `x**0 = 1` for any x, even
NaN). C standards before C99 left this implementation-defined. glibc and
musl currently return 1.0; older Solaris libm and some Windows CRT
implementations return NaN. The folder forwards the host result unchanged
(no special-case here).

### C. `atan2(±0, ±0)` (line 3395-3401)
Already special-cased to skip folding *only* when both inputs are zero
(`Solaris` exception). But the surrounding `Op1V.isZero() && Op2V.isZero()`
check only suppresses the *all-zero* case; the related `atan2(±0, x)` and
`atan2(x, ±0)` boundary behaviors still go through host libm and differ
across implementations (some libms get the sign of the result wrong for
`atan2(-0, -1) = -π` vs `+π`).

### D. Double-rounding for half/float
`ConstantFoldFP` (line 2263) always evaluates at `double` host precision,
then `GetConstantFoldFPValue` converts back to `Ty` (half/float). For half
or float result types the value is **doubly rounded**:

  1. Inside `pow`/`sin`/etc. at `double` precision (libm rounding).
  2. `convert(double→half)` or `convert(double→float)` (lines 2197–2200).

The doubly-rounded result can disagree by 1 ULP from a single
correctly-rounded half-precision evaluation. For half (`fp16`) this is
quite common with transcendentals because `double` mantissa underflows /
intermediate cancellation perturbs the half result. The runtime `__half`
implementations on x86 (and `_Float16` libm extensions) are not doubly
rounded, so constant-folded `@llvm.sin.f16(C)` differs from
`sinf16(C)` at runtime.

### E. `ConstantFoldBinaryFP` exception-status check is *not* present
Compare `ConstantFoldFP` (line 2263) which clears and tests fenv exceptions
(`llvm_fenv_clearexcept`/`llvm_fenv_testexcept`) and *bails out* on
exception:

```cpp
llvm_fenv_clearexcept();
double Result = NativeFP(Input.convertToDouble());
if (llvm_fenv_testexcept()) {
  llvm_fenv_clearexcept();
  return nullptr;
}
```

`ConstantFoldBinaryFP` (line 2300) does the same clear/test — actually OK
there. Good. But it shares the host-libm variance issue.

## Why this is in-scope for x86

* The constant-folded IR is then handed to the X86 backend, which simply
  emits the precomputed bits. Differing host-libm bits → different `.text`
  → different runtime behavior.
* The bug surfaces visibly as a non-deterministic / non-reproducible build:
  build LLVM on glibc-2.34, fold `pow(-1, INF)` → `1.0`; build LLVM on a
  toolchain whose `pow` returns NaN for the same args → `NaN`. The same
  source.bc produces different x86 `.o`.
* Even more user-visible: cross-compiling LLVM to a target whose libm
  differs from the build host produces *worse* host-libm divergence
  (the compiler embeds the *build* host's libm results, not the target's
  runtime libm results, breaking `-O0` vs `-O2` equivalence).

## Status

* Source-confirmed (single-FIXME-style carve-out for `atan2(0,0)` is the
  only existing mitigation; the rest of the host-libm calls are
  uncontrolled).
* Not a single miscompile — a class of miscompiles. Each transcendental on
  each boundary input (denormals, ±∞, NaN, signed-zero ties) is a separate
  micro-bug.

## Fix sketch

* Use `APFloat::*` correctly-rounded transcendentals where available (now
  exists for `sqrt`, partly for `log` via `logf128`).
* For each fold, define expected IEEE result for boundary cases (±0, ±∞,
  NaN, denormal) and special-case them *before* calling host libm.
* For half/float results, evaluate at *target* precision via `APFloat`,
  not at `double` host precision.

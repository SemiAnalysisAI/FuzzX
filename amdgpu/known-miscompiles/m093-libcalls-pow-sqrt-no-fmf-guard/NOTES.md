# m093: `AMDGPULibCalls::fold_pow` rewrites `pow(x, ±0.5)` to `sqrt`/`rsqrt` without fast-math gating, losing IEEE corner cases

*Discovery method: code inspection.*  Found by reading
`AMDGPULibCalls::fold_pow` in
`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPULibCalls.cpp`
(lines 936-950).

## The bug

```cpp
if (CF && (CF->isExactlyValue(0.5) || CF->isExactlyValue(-0.5))) {
  // pow[r](x, [-]0.5) = sqrt(x)
  bool issqrt = CF->isExactlyValue(0.5);
  if (FunctionCallee FPExpr =
          getFunction(M, AMDGPULibFunc(issqrt ? AMDGPULibFunc::EI_SQRT
                                              : AMDGPULibFunc::EI_RSQRT,
                                       FInfo))) {
    ...
    Value *nval = CreateCallEx(B, FPExpr, opr0,
                               issqrt ? "__pow2sqrt" : "__pow2rsqrt");
    replaceCall(FPOp, nval);
    return true;
  }
}

if (!isUnsafeFiniteOnlyMath(FPOp))
  return false;
```

The fold returns **before** the `isUnsafeFiniteOnlyMath` guard, so it
fires for plain (non-fast) `pow`/`powr`/`pown` too.

Per C99 §F.10.4.4 and IEEE 754-2008 `powr`:

* `pow(-Inf, 0.5)` = `+Inf` (`pow(-Inf, y)` with `y > 0` non-odd-integer)
* `pow(-0.0, 0.5)` = `+0.0` (`pow(±0, y)` with `y > 0` non-integer)

But:

* `sqrt(-Inf)`  = NaN per IEEE; `llvm.sqrt.f32` follows the same rule.
* `sqrt(-0.0)`  = `-0.0` per IEEE (sign of zero preserved);
  `llvm.sqrt.f32` likewise.

So the fold loses the correct OpenCL/C99 `pow` semantics whenever the
input might be negative zero or negative infinity.  The generic
`InstCombine` `pow -> sqrt` fold (`InstCombineCalls.cpp`) correctly
gates on `nnan` + `ninf` (or `nsz`) for exactly this reason; the
AMDGPU version forgot the guard.

## Reproducer

```bash
amdgpu/known-miscompiles/run_ll_reproducer.sh \
  amdgpu/known-miscompiles/m093-libcalls-pow-sqrt-no-fmf-guard/reduced.ll
```

The kernel calls `_Z3powff(x, 0.5)` with two inputs: `0xff800000`
(`-Inf`) and `0x80000000` (`-0.0`).  At `-O0` the pass isn't run and
the in-module `_Z3powff` definition returns the C99 answer.  At `-O2`
the call is rewritten to `_Z4sqrtf` (also in-module, wrapping
`llvm.sqrt.f32`), and the IEEE-`sqrt` corner cases bite:

```text
[0] input=0xff800000 O0=0x7f800000 O2=0xffc00000 mismatch=true   ; -Inf:  +Inf vs NaN
[1] input=0x80000000 O0=0x00000000 O2=0x80000000 mismatch=true   ; -0.0:  +0.0 vs -0.0
any_mismatch=true
```

## How a fix should look

Gate the rewrite on the call's fast-math flags:

```cpp
if (CF && (CF->isExactlyValue(0.5) || CF->isExactlyValue(-0.5)) &&
    FPOp->hasNoNaNs() && FPOp->hasNoInfs() && FPOp->hasNoSignedZeros()) {
  ...
}
```

(or use the broader `FPOp->isFast()`, matching the rest of `fold_pow`).
Alternatively, special-case the negative-x inputs by emitting
`copysign(sqrt(fabs(x)), x)`-style fixups plus the appropriate
`isnan`/`isinf` selects.

## Why the fuzzer doesn't see it

* The fold requires a constant `±0.5` exponent **and** a module-visible
  *definition* of `_Z4sqrtf` / `_Z5rsqrtf` (`AMDGPULibFunc::getFunction`
  rejects declaration-only callees).  The fuzzer rarely emits both in
  the same module.
* The interpreter oracle is skipped for any module containing
  `amdgcn.*` intrinsics, so even an emitted mismatch wouldn't be
  caught.

## Other folds spot-audited and ruled out

* `evaluateScalarMathFunc` (line 1790) does use host-side `double`
  math for f32 args, but it is gated by
  `canIncreasePrecisionOfConstantFold` -> `FPOp->isFast()`
  (line 454-458), so the "double precision applied to f32 call"
  mismatch only fires when the user opted in with fast-math.
* `fold_sincos` (line 1692) bails on `ConstantData` operands.
* `tryReplaceLibcallWithSimpleIntrinsic` for `exp`/`exp2`/`log`/`log2`/
  `log10` (lines 641-665) all bail if `FMF.none()` and pass
  `FMF.approxFunc()` as the minsize-bypass flag.
* The `pow(x, 2.0) -> x*x` / `pow(x, -1.0) -> 1.0/x` / `pow(x, 0)
  -> 1.0` folds (line 900-934) are IEEE-correct.
* `tbl_*` table folds (lines 200-342) only match exact inputs that
  produce the exact correct constant output.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces: -Inf -> NaN, -0.0 -> -0.0. |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Reproduces (same fold present). |

Not a HEAD-only regression -- this fold has been in the AMDGPU library
calls pass for years.

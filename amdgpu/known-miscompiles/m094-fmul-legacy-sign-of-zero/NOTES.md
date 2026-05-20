# m094: `amdgcn.fmul.legacy` / `amdgcn.fma.legacy` -> `fmul` / `fma` drops sign-of-zero

*Discovery method: code inspection.* Found by reading
`AMDGPUInstCombineIntrinsic.cpp`.

`V_MUL_LEGACY_F32` is defined (AMD ISA, gfx950 §) as
`D = (S0 == 0.0 || S1 == 0.0) ? +0.0 : S0 * S1`. The "+0.0" is positive
regardless of the operand signs. InstCombine's `canSimplifyLegacyMulToMul`
helper, used by both the `amdgcn_fmul_legacy` and `amdgcn_fma_legacy`
folds, decides it is safe to rewrite the legacy intrinsic as a regular
`fmul` / `fma` whenever either operand can be proven *not* to be `0`,
`Inf` or `NaN`. The intent is "if neither operand is 0, the legacy zero
clause doesn't fire, so a regular fmul produces the same answer."

The problem: `match(Op0, m_FiniteNonZero())` only proves that **one**
operand is non-zero (and finite). The **other** operand can still be
`+0.0` or `-0.0` at runtime. When it is `±0.0`, the two operations
disagree on the *sign* of the result:

* Legacy: `(-2.0) * (+0.0) = +0.0`  (legacy zero clause forces +0).
* IEEE fmul: `(-2.0) * (+0.0) = -0.0`  (sign of the product is XOR of
  the operand signs).

Sign-of-zero is architecturally visible: bitcast, `signbit`, `1/x`,
`copysign`, and any later store all see the wrong value.

## Reproducer

```bash
amdgpu/known-miscompiles/run_ll_reproducer.sh \
  amdgpu/known-miscompiles/m094-fmul-legacy-sign-of-zero/reduced.ll
```

```text
input=0x00000000
O0=0x00000000        ; correct: legacy gives +0.0
O2=0x80000000        ; wrong:   IEEE fmul gives -0.0
mismatch=true
```

Reproduces on both LLVM HEAD with local PR patches and ROCm 7.1.1
(`/opt/rocm-7.1.1`) clang-20.

## Root Cause

`AMDGPUInstCombineIntrinsic.cpp:398`:

```cpp
bool GCNTTIImpl::canSimplifyLegacyMulToMul(const Instruction &I,
                                           const Value *Op0, const Value *Op1,
                                           InstCombiner &IC) const {
  // The legacy behaviour is that multiplying +/-0.0 by anything, even NaN or
  // infinity, gives +0.0. If we can prove we don't have one of the special
  // cases then we can use a normal multiply instead.
  if (match(Op0, PatternMatch::m_FiniteNonZero()) ||
      match(Op1, PatternMatch::m_FiniteNonZero())) {
    // One operand is not zero or infinity or NaN.
    return true;
  }
  ...
```

The comment is correct ("if we can prove we don't have one of the
special cases"), but the body only proves it for **one** operand. The
zero-with-sign case is still a "special case" of the legacy contract,
because legacy returns `+0.0` while IEEE preserves the XOR-of-signs.

Both intrinsic folds rely on this helper:

* `case Intrinsic::amdgcn_fmul_legacy` at line 2015 calls
  `IC.Builder.CreateFMulFMF(Op0, Op1, &II)`.
* `case Intrinsic::amdgcn_fma_legacy` at line 2040 swaps the intrinsic
  for `Intrinsic::fma`.

Either path produces a `-0.0` where the legacy intrinsic produces
`+0.0`, whenever the runtime operand is `-0.0` (or `+0.0` with a
negative finite-nonzero constant on the other side).

The existing test
`llvm/test/Transforms/InstCombine/AMDGPU/fmul_legacy.ll` line 28
encodes exactly this transform as the "expected" behavior, so it would
need updating along with the fix.

## Fix Sketch

`canSimplifyLegacyMulToMul` is only safe when the intrinsic carries
`nsz` (sign of zero may be ignored), or both operands are known to be
non-zero (so the zero clause never fires), or the intrinsic's
single-use chain demonstrably ignores sign of zero. Minimal fix:

```cpp
if (I.hasNoSignedZeros())
  return existing-finite-nonzero-check();
// Otherwise require both operands to be non-zero.
return isKnownNeverZeroFloat(Op0, ...) && isKnownNeverZeroFloat(Op1, ...);
```

Alternatively, after rewriting to `fmul`/`fma`, mask the sign with
`copysign(result, 0.0)` so the `+0` polarity is preserved.

## Why the fuzzer doesn't see it

* `amdgcn.fmul.legacy` and `amdgcn.fma.legacy` are not in the AMDGPU IR
  fuzzer's emit set, so it never generates the intrinsic in the first
  place. (Search the IR generator for `fmul_legacy` / `fma_legacy` --
  no hits.)
* Even if it were emitted, the bug requires *one* constant operand
  (finite-nonzero, to enter `canSimplifyLegacyMulToMul`'s "true"
  branch) and the other operand to be `±0.0` at runtime. The
  interpreter oracle is disabled for `amdgcn.*` intrinsics, so it
  cannot catch the divergence either.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with local PR patches | Reproduces: `O0=0x00000000`, `O2=0x80000000`. |
| ROCm 7.1.1 (`/opt/rocm-7.1.1` clang-20) | Reproduces: `O0=0x00000000`, `O2=0x80000000`. |

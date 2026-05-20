# m092: `performSelectCombine` `select (fcmp one x, K), other, K` -> `..., x` fold drops NaN-input semantics

*Discovery method: code inspection.*  Found by reading
`SITargetLowering::performSelectCombine` in
`SIISelLowering.cpp` (around line 18306).

## The bug

`SITargetLowering::performSelectCombine` (`SIISelLowering.cpp:18335-18374`)
contains the fold:

```
select (fcmp one x, K), other, K  ->  select (fcmp one x, K), other, x
```

(plus the parallel `SETOEQ` arm that rewrites the *true* value).  The
fold's stated motivation is avoiding two materialisations of the
constant `K`.  It correctly guards the **constant** side:

```cpp
if (isFloatingPoint) {
  const APFloat &Val = cast<ConstantFPSDNode>(ConstVal)->getValueAPF();
  if (!Val.isNormal() || Subtarget->getInstrInfo()->isInlineConstant(Val))
    return SDValue();
}
...
SDValue SelectRHS =
    (isNonEquality && FalseVal == ConstVal) ? ArgVal : FalseVal;
```

But it does **not** guard the non-constant operand `x`.

When `x` is a NaN:

* Original IR: `fcmp one NaN, K = false` (ordered fails on NaN), so
  `select false, other, K = K`.
* Folded IR: same `fcmp one NaN, K = false`, but now
  `select false, other, x = x = NaN`.

The two diverge: the optimised code leaks the NaN through to the
result.

The symmetric `SETOEQ` arm rewrites the **true** value to `x`; the
true arm is never taken when `x = NaN`, so that arm is safe.  The bug
is `SETONE` only.

## Reproducer

```bash
amdgpu/known-miscompiles/run_ll_reproducer.sh \
  amdgpu/known-miscompiles/m092-select-fcmp-one-nan-arg/reduced.ll
```

Observed on LLVM HEAD with the local PR patches, `gfx950`:

```text
[0] input=0x7fc00000 O0=0x402df850 O2=0x7fc00000 mismatch=true
[1] input=0x40000000 O0=0x402df850 O2=0x7fc00000 mismatch=true
any_mismatch=true
```

The kernel does `select (fcmp one x, K), other, K` with
`K = 0x402df850` (2.71875, normal, non-inline-immediate).  Lane 0
passes `x = qNaN` (`0x7fc00000`); `-O0` correctly stores `K`, `-O2`
incorrectly stores the NaN that `x` carried.

## How a fix should look

Either gate the `SETONE` rewrite on the compare carrying `nnan`:

```cpp
if (isNonEquality && FalseVal == ConstVal &&
    !cast<SDNode>(Cond)->getFlags().hasNoNaNs())
  return SDValue();
```

or restrict the fold to the equality arm (`SETOEQ` / `SETEQ`), where
the rewrite happens on the true value and NaN never reaches it.

Existing LIT tests in `test/CodeGen/AMDGPU/select-cmp-shared-constant-fp.ll`
cover the **constant**-NaN / Inf / zero / subnormal / inline-immediate
cases (see `no_fold_one_f32_nan` at line 277) but none of them exercise
the **input**-NaN case.

## Why the fuzzer doesn't see it

The fold is a HEAD-only addition (PR pr-198373 merged after ROCm 7.1.1's
clang-20 snapshot).  The fuzzer's static IR emitters rarely pair an
`fcmp one` against a normal non-inline f32 constant with an argument
that could realistically be NaN, and the interpreter oracle is skipped
for kernels using `volatile` loads (the reproducer uses
`load volatile` to defeat constant propagation), so the divergence
wouldn't be caught.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces: `O0=0x402df850`, `O2=0x7fc00000`. |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Does NOT reproduce -- fold not yet present. |

Confirms HEAD-only regression, same flavour as `m076`.

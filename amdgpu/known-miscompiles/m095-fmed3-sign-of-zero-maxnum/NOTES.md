# m095: `amdgcn.fmed3` constant fold loses sign-of-zero via `maxnum` tie-break

*Discovery method: code inspection.*  Found during a third audit of
`AMDGPUInstCombineIntrinsic.cpp` for sibling shapes to m094 (legacy-op
sign-of-zero loss).

## The bug

`AMDGPUInstCombineIntrinsic.cpp:53-68` -- helper `fmed3AMDGCN`, invoked
from the all-constant `amdgcn.fmed3` case at line 1593-1596:

```cpp
static APFloat fmed3AMDGCN(const APFloat &Src0, const APFloat &Src1,
                           const APFloat &Src2) {
  APFloat Max3 = maxnum(maxnum(Src0, Src1), Src2);
  APFloat::cmpResult Cmp0 = Max3.compare(Src0);
  if (Cmp0 == APFloat::cmpEqual)
    return maxnum(Src1, Src2);
  APFloat::cmpResult Cmp1 = Max3.compare(Src1);
  if (Cmp1 == APFloat::cmpEqual)
    return maxnum(Src0, Src2);
  return maxnum(Src0, Src1);
}
```

`APFloat::compare` treats `+0` and `-0` as equal (`cmpEqual`), so for
`fmed3(-0, -0, +0)` the first branch fires (since `Max3 = +0`
compare-equals `Src0 = -0`) and the fold returns `maxnum(Src1=-0,
Src2=+0)`.  `APFloat::maxnum` (per `APFloat.h:1693`) "treats +0 as
ordered greater than -0", so it returns `+0`.

The fold therefore collapses `fmed3(-0, -0, +0)` -> `+0`.

Hardware `v_med3_f32(-0, -0, +0)` returns the actual median by sort
order including sign-of-zero -- the sorted triple is `{-0, -0, +0}`,
so the median is `-0` (`0x80000000`).  Confirmed by the `-O0` result.

This is a runtime miscompile of a fully-constant intrinsic.

## Reproducer

```bash
amdgpu/known-miscompiles/run_ll_reproducer.sh \
  amdgpu/known-miscompiles/m095-fmed3-sign-of-zero-maxnum/reduced.ll
```

Observed on `gfx950`:

```text
input=0x00000000
O0=0x80000000   ; v_med3_f32(-0, -0, +0) = -0 (HW median)
O2=0x00000000   ; constant fold returns +0 (sign-of-zero lost)
mismatch=true
```

Reproduces identically on the local LLVM HEAD `build/llvm-fuzzer` and
on ROCm 7.1.1 `clang-20`.

## Generalisation

The bug is not specific to that one input triple.  Any input triple
that lands on a `maxnum`-tie branch with sign-of-zero candidates is
affected -- e.g. `fmed3(-0, +0, -0)` and `fmed3(+0, -0, -0)` exercise
the second and third arms of the helper symmetrically.

## How a fix should look

When `Max3.compare(SrcN) == cmpEqual` ties at zero, the fold cannot
break the tie via `maxnum` (which is sign-agnostic in the wrong
direction).  Two reasonable fixes:

1. Detect the tie-at-zero case and pick the actual median directly
   (e.g., if the three values are not strictly distinct, return any
   operand whose magnitude matches the median rank).
2. Bail out of the fold whenever any two operands compare equal but
   differ in bitwise sign-of-zero -- let the hardware compute it.

## Why the fuzzer doesn't see it

* `amdgcn.fmed3` is in the AMDGPU fuzzer's intrinsic set, but the
  fuzzer never calls it with three constant operands (it always uses
  runtime VGPR values).  The fold is constant-only.
* The interpreter oracle is disabled for any module containing
  `amdgcn.*` intrinsics, so even an emitted constant fmed3 wouldn't
  be checked against the spec.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces: `O0=0x80000000`, `O2=0x00000000`. |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Reproduces. |

Not a HEAD-only regression -- the helper has been in the AMDGPU
InstCombine for years.

## Relation to other m-bugs in this file

* `m074` (fmed3 NaN polarity in IEEE-off mode) -- different code path
  (`isNaN`/`isInfinity` branches of `simplifyAMDGCNInstCombineIntrinsic`),
  not the constant-fold helper here.
* `m094` (fmul.legacy/fma.legacy sign-of-zero) -- same sign-of-zero
  family, different fold.

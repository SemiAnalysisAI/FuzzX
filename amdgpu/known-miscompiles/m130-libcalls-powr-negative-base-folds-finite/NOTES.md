# m130: `AMDGPULibCalls::fold_pow` constant-exponent shortcuts ignore the OpenCL `powr` negative-base spec

*Discovery method: code inspection.*  Companion of m093 (which covers
`pow(x, ±0.5)`-without-fmf; this one covers `powr` semantics violations
across multiple exponent shortcuts).

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPULibCalls.cpp:900-1005`
(`AMDGPULibCalls::fold_pow`).

OpenCL/IEEE `powr(x, y)` requires:

| (x, y) | OpenCL `powr` result |
| --- | --- |
| `x < 0`, any y      | NaN |
| `NaN, 0`            | NaN |
| `+0, 0` / `-0, 0`   | NaN |

(`powr` explicitly diverges from C `pow` on these corners -- base must
be `>= 0`.)

The constant-exponent shortcuts at lines 900-1005 do NOT check
`FInfo.getId()`, so they fire for `EI_POWR` / `EI_POWR_FAST` too,
producing finite values:

* Line 900-908: `powr(x, 0)  -> 1.0`         -- wrong for `x = NaN, ±0`
* Line 910-915: `powr(x, 1)  -> x`           -- OK
* Line 916-923: `powr(x, 2)  -> x*x`         -- wrong for `x < 0`
* Line 924-934: `powr(x, -1) -> 1.0/x`       -- wrong for `x < 0`
* Line 936-950: `powr(x, ±0.5) -> sqrt/rsqrt` -- m093 covers same shape
                                                  for `pow`; same here
                                                  for `powr`
* Line 969-1005: `powr(x, abs(c) <= 12 int)` unsafe-math expansion ->
  binary multiplication chain -- wrong for `x < 0`

## Reproducer

`reduced.ll`:

```llvm
declare protected float @_Z4powrff(float, float)

define amdgpu_kernel void @t(ptr addrspace(1) %out, float %x) {
  %r = call float @_Z4powrff(float %x, float 2.0)
  store float %r, ptr addrspace(1) %out
  ret void
}
```

`opt -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950 -passes=amdgpu-simplifylib -S reduced.ll`
shows the fold replacing the `powr` call with `fmul x, x`.

Runtime values:

| `x` | OpenCL `powr(x, 2)` | observed (fold) |
| --- | --- | --- |
| `-2.0` | NaN | `4.0` (`0x40800000`) |
| `-1.0` | NaN | `1.0` (`0x3F800000`) |
| `-10.0` | NaN | `100.0` (`0x42C80000`) |

Companion test `powr(x, -1)` for `x < 0`: NaN expected, observed
`-0.5` / `-1.0` / `-0.1`.

Companion test `powr(x, 0)` for `x in {NaN, +0, -0}`: NaN expected,
observed `1.0` for all three.

## Suggested fix

Gate every early shortcut in `fold_pow` on
`FInfo.getId() != EI_POWR && FInfo.getId() != EI_POWR_FAST`, or bail
upfront for `EI_POWR` when the base is not provably `>= 0`:

```cpp
if (FInfo.getId() == AMDGPULibFunc::EI_POWR ||
    FInfo.getId() == AMDGPULibFunc::EI_POWR_FAST) {
  if (!cannotBeOrderedLessThanZeroFP(opr0, B.getDataLayout(),
                                     /*Depth=*/0))
    return false;   // bail; correct powr(x<0, y) = NaN
}
```

Same fix needed in the `abs(c) <= 12` unsafe-math expansion at line 972
(the `needabs`/`needcopysign` adjustment there assumes `pow`/`pown`
semantics, not `powr`).

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (fold rewrites to fmul/fdiv). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Same defect (predates AMDGPULibFunc rewrite). |

## Why the fuzzer hasn't caught it

Same as m093: AMDGPULibCalls requires module-visible `_Z4powrff` (or
`_Z4powrDv2_fS_`) declarations; `AMDGPULibFunc::getFunction` rejects
mere decls so the IR fuzzer's randomly-emitted calls miss the trigger.
Per `MEMORY.md` (Prefer-random-over-idioms), the right hook is to
have the OpenCL-style emitter occasionally inject the canonical
`_Z4powr*` mangled name as a body-only declaration + use.

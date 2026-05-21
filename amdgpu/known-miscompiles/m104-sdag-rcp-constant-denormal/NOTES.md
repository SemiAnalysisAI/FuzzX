# m104: SDAG `performRcpCombine` constant fold ignores f32 denormal mode (SDAG twin of m075/m077)

*Discovery method: code inspection.* Sibling shape to m075 (output-denormal
not flushed) and m077 (input-denormal not flushed) but at the SDAG layer,
post-InstCombine, so the m075/m077 fixes don't cover it.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUISelLowering.cpp:5549-5558`:

```cpp
SDValue AMDGPUTargetLowering::performRcpCombine(SDNode *N,
                                                DAGCombinerInfo &DCI) const {
  const auto *CFP = dyn_cast<ConstantFPSDNode>(N->getOperand(0));
  if (!CFP)
    return SDValue();

  // XXX - Should this flush denormals?
  const APFloat &Val = CFP->getValueAPF();
  APFloat One(Val.getSemantics(), "1.0");
  return DCI.DAG.getConstantFP(One / Val, SDLoc(N), N->getValueType(0));
}
```

The fold computes `1.0 / Val` in full APFloat precision and never consults
`MF.getDenormalMode(VT.getFltSemantics())`.  The literal comment is *"XXX -
Should this flush denormals?"*.  `SITargetLowering::performRcpCombine`
(`SIISelLowering.cpp:15476-15494`) handles the undef and f16-rsq cases
then delegates here.

The fold has two flush bugs:

1. **Output-denormal not flushed** (m075 shape): for `|Val| > 2^126`, the
   true `1/Val` is subnormal in f32.  Hardware `v_rcp_f32` under
   `denormal-fp-math-f32=preserve-sign,preserve-sign` flushes the result
   to `±0`.  The constant fold returns the subnormal.

2. **Input-denormal not flushed** (m077 shape): for `|Val| < 2^-126`,
   hardware first flushes `Val` to `±0` then returns `±Inf`.  The
   constant fold reciprocates the subnormal directly and returns a finite
   number near `2^127`.

## How the buggy shape arises

The fold is reachable from valid IR without `@llvm.amdgcn.rcp` -- through
the `lowerFastUnsafeFDIV` shortcut at `SIISelLowering.cpp:13104-13125`:

```cpp
// 1.0 / x  ->  rcp(x)    (under `afn`)
```

`fdiv afn float 1.0, C` becomes `AMDGPUISD::RCP(C)`, then
`performRcpCombine` sees the constant operand and folds it without flush.

(The InstCombine sibling m075/m077 only fire on direct
`@llvm.amdgcn.rcp(C)`, not on `1.0/C` -- it goes straight to SDAG and
hits this path.)

## Reproducer

`reduced.ll`:

```llvm
%r  = fdiv afn float 1.0, 0x47E0000000000000   ; 2.0**127
store float %r, ptr addrspace(1) %out
```

* Expected with `denormal-fp-math-f32="preserve-sign,preserve-sign"`:
  hardware `v_rcp_f32` would emit `+0.0` (`0x00000000`).
* Observed (SDAG fold fires): `0x00400000` (subnormal `2^-127`).

The kernel attribute `denormal-fp-math-f32="preserve-sign,preserve-sign"`
asks for FTZ semantics on f32; the fold returns a subnormal anyway.

For the m077-shape: replace the divisor with `0x3810000000000000`
(= subnormal `2^-127` in f32).  Hardware would flush the input to `+0`
and produce `+Inf`; the constant fold returns a finite normal `~2^127`.

## Suggested fix

Mirror m075/m077: consult the kernel's denormal mode.

```cpp
const DenormalMode FPMode = DCI.DAG.getMachineFunction()
    .getDenormalMode(Val.getSemantics());

APFloat Input = Val;
if (Input.isDenormal() &&
    FPMode.Input != DenormalMode::IEEE) {
  // Flush input to signed zero.
  Input = APFloat::getZero(Val.getSemantics(), Val.isNegative());
}

APFloat One(Val.getSemantics(), "1.0");
APFloat R = One / Input;

if (R.isDenormal() &&
    FPMode.Output != DenormalMode::IEEE) {
  R = APFloat::getZero(Val.getSemantics(), R.isNegative());
}

return DCI.DAG.getConstantFP(R, SDLoc(N), N->getValueType(0));
```

Plus the same fix in `lowerFastUnsafeFDIV` (which feeds this fold and
could short-circuit the fold's denormal blindness).

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Fold fires; output is subnormal under FTZ mode. |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Same fold present (TODO comment unchanged). |

Not a HEAD-only regression.  The TODO at line 5555 has been there for
years.

## Why the fuzzer doesn't see it

* The current FP emitter generates `fdiv` with `arcp`/`fast` but rarely
  the `(1.0, C)` shape with a literal constant near the f32 extremes.
* The interpreter oracle is currently skipped for kernels with `afn`
  fdiv where the divisor is a 64-bit-encoded f32 constant.
* Per `MEMORY.md` (Prefer-random-over-idioms), the right hook is to
  add `2^127`, `2^-127`, and similar extreme f32 constants to the
  constant pool and let `fdiv afn 1.0, C` emerge naturally.

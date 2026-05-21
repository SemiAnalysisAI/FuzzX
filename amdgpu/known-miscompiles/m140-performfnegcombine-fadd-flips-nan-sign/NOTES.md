# m140: `performFNegCombine` FADD arm: `(fneg (fadd x, y)) -> (fadd -x, -y)` flips NaN sign

*Discovery method: code inspection from `performFNegCombine` arm
audit; HW-verified on gfx950.*

Direct sibling of m139 (FMA arm of the same combine).  The combine
is gated only on `nsz`; needs `nnan`.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUISelLowering.cpp:5273-5297`
(`AMDGPUTargetLowering::performFNegCombine`, FADD case):

```cpp
case ISD::FADD: {
  if (!mayIgnoreSignedZero(N0))                  // <-- nsz only
    return SDValue();

  // (fneg (fadd x, y)) -> (fadd (fneg x), (fneg y))
  SDValue LHS = N0.getOperand(0), RHS = N0.getOperand(1);
  ...
  return DAG.getNode(ISD::FADD, SDLoc(N), VT, NegLHS, NegRHS, N0->getFlags());
}
```

`fneg` on a NaN flips exactly the sign bit (LangRef / IEEE-754).
`fadd(x, y)` returning NaN (from e.g. `+inf + -inf`, or NaN
propagated from a NaN input) selects a payload/sign by an
implementation-defined rule.  Substituting `-x, -y` for `x, y`
changes which operand is NaN (or which operand the NaN sign comes
from) and therefore changes the result NaN's sign bit.

`nsz` permits ignoring sign-of-zero in the result; it does NOT
license changing NaN payload/sign.  The combine needs `nnan` (or a
guard that neither operand can produce NaN).

## Reproducer

`reduced.ll` (v2f16):

```llvm
%v0 = fadd nsz <2 x half> %in0, %in1                ; fadd of NaN/Inf
%v6 = fsub <2 x half> <half -0.0, half -0.0>, %v0   ; = fneg(v0)
store i32 (bitcast v6 to i32), ptr addrspace(1) %out
```

Inputs:
* `in0 = 0xfe00fc00` (top: -qNaN payload 0; bottom: -Inf)
* `in1 = 0x7c007c00` (both: +Inf)

So `v0 = fadd(in0, in1)`:
* top half:    `fadd(-qNaN, +Inf)` -> NaN (HW propagates -qNaN).
* bottom half: `fadd(-Inf, +Inf)`  -> NaN (sign per HW).

Then `v6 = fneg(v0)`:
* expected per LangRef: flip both sign bits exactly.

```
=== -O0 ===
v_pk_add_f16 v1, v1, v2
v_xor_b32_e64 v1, v1, s2          ; s2 = 0x80008000 -- explicit fneg via xor

stores 0x7e007e00                  ; both lanes have NaN sign flipped per fneg

=== -O2 ===
v_pk_add_f16 v1, v1, v2 neg_lo:[1,1] neg_hi:[1,1]
                                    ; FADD arm fired: NegLHS=fneg(in0), NegRHS=fneg(in1)
                                    ; explicit fneg deleted; sign comes from HW fadd of
                                    ; sign-flipped inputs

stores 0x7e00fe00                  ; bottom-half NaN sign NOT flipped (kept negative)
```

The same IR yields different NaN sign at the bottom lane between O0
and O2.  LangRef requires `fneg` to flip the sign bit precisely; the
substitution `-fadd(x,y) -> fadd(-x,-y)` does NOT preserve that
guarantee for NaN-producing fadds.

`run_ll_reproducer.sh`:

```
O0=0x7e007e00 O2=0x7e00fe00 mismatch=true
```

## Suggested fix

Replace the `nsz`-only gate with `nsz && nnan`:

```cpp
case ISD::FADD: {
  if (!mayIgnoreSignedZero(N0))
    return SDValue();
  if (!N0->getFlags().hasNoNaNs())     // <-- ADD
    return SDValue();
  ...
}
```

The identical fix applies to the FMA arm (m139).  An audit of the
entire `performFNegCombine` switch should be folded into one PR; the
following arms in the same function also have `nsz`-only gates with
the same theoretical defect:

* FMINNUM/FMAXNUM/FMINNUM_IEEE/FMAXNUM_IEEE/FMINIMUM/FMAXIMUM/FMINIMUMNUM/FMAXIMUMNUM/FMIN_LEGACY/FMAX_LEGACY (lines 5349-5382) -- HAVE no FMF gate at all.  On gfx950 verified to not produce HW-observable divergence for `v_pk_min/max_f16` (symmetric NaN-payload-selection), but is unsound on architectures whose NaN propagation depends on src0/src1 slot.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (`O0=0x7e007e00, O2=0x7e00fe00`). |
| ROCm 7.1.1 | Reproduces (same combine, same gate). |

## Why the fuzzer hasn't caught it

* Same as m139: default FuzzX scalar emitter rarely produces
  NaN-producing FADD chains observed via `fneg`.  Per `MEMORY.md`
  (Prefer-random-over-idioms), enriching the random FP emitter to
  bias toward NaN-input / Inf-Inf chains with terminal `fneg` would
  surface the m107/m120/m127/m128/m139/m140 family from a single
  pass.
* The v2f16 NaN-biased fuzz harness caught the m139 (FMA arm) bug
  on its 20th seed; m140 found via direct sibling-arm audit.

# m120: `performFMulCombine` `fmul x, (select y, -A, -B)` -> `ldexp(fneg x, ...)` flips NaN sign

*Discovery method: code inspection.*  Inverse shape of m107 (m107:
original IR flips NaN sign that the fold preserves; m120: original IR
preserves NaN sign that the fold flips).

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:17719-17723`
(`performFMulCombine`):

```cpp
LHS = TrueNode->isNegative()
          ? DAG.getNode(ISD::FNEG, SL, VT, LHS, LHS->getFlags())
          : LHS;
return DAG.getNode(ISD::FLDEXP, SL, VT, LHS, SelectNode, N->getFlags());
```

When both select arms are negative powers of two, the combine wraps
`x` in `FNEG` so the resulting ldexp can be expressed as
`ldexp(-x, log2(|A|))`.  There is NO `nnan` guard.

Under AMDGPU HW semantics:

* `v_mul_f64(NaN, -K)` preserves the input NaN's sign bit (HW
  NaN-propagation: NaN payload + sign passed through).
* `v_ldexp_f64(-x, k)` lowers `FNEG` into the VOP3 NEG source
  modifier, which XORs the sign bit **before** ldexp sees the
  operand, then ldexp preserves that flipped sign in its NaN
  propagation.

Net: for any NaN-valued `x`, original IR yields a NaN with the input
sign; folded form yields a NaN with the flipped sign.

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @t(ptr addrspace(1) %out, double %x, i1 %y) {
  %sel = select i1 %y, double -2.0, double -4.0
  %m = fmul double %x, %sel
  store double %m, ptr addrspace(1) %out
  ret void
}
```

Codegen with `clang -mcpu=gfx950`:

```asm
=== -O0 ===
v_mul_f64 v[2:3], s[2:3], v[2:3]   ; preserves NaN sign

=== -O2 ===
v_ldexp_f64 v[0:1], -s[2:3], v0    ; NEG src-modifier flips NaN sign
```

For `x = 0x7FF8000000000000` (+qNaN), `y = true`:

* O0: stores `0x7FF8000000000000` (+qNaN preserved).
* O2: stores `0xFFF8000000000000` (-qNaN -- sign flipped).

Same shape applies to f32 and f16 in divergent contexts (the
`hasSALUFloatInsts()` early-exit only blocks scalar f32/f16, so the
combine still fires for divergent x of any FP type):

```asm
v_ldexp_f32 v1, -v1, s2    ; f32 divergent case
v_ldexp_f16 v1, -v1, s2    ; f16 divergent case
```

## Suggested fix

Gate the FNEG-LHS arm on `nnan`:

```cpp
LHS = TrueNode->isNegative()
          ? (N->getFlags().hasNoNaNs()
                 ? DAG.getNode(ISD::FNEG, SL, VT, LHS, LHS->getFlags())
                 : SDValue())   // bail
          : LHS;
if (!LHS) return SDValue();
```

Or bail from the whole combine when both arms are negative and
`!N->getFlags().hasNoNaNs()`.  The non-negative path is unaffected
(both `v_mul_f64(NaN, K)` and `v_ldexp_f64(NaN, k)` preserve NaN sign
identically).

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (`v_ldexp_f64 -s[2:3], v0`). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Same fold present. |

## Why the fuzzer hasn't caught it

* Same as m107: NaN bit-patterns rarely seed arithmetic operand
  positions.
* Per `MEMORY.md` (Prefer-random-over-idioms), weight
  `0x7FF8000000000000`, `0xFFF8000000000000`, and
  `0x7FF0000000000001` higher in the f64 constant pool, plus ensure
  `fmul-select(neg-pow2, neg-pow2)` shapes can pick them as LHS.

## LowerFLDEXP feeds integer exponent to SCALEF (missing SINT_TO_FP) when widening to 512-bit

`llvm/lib/Target/X86/X86ISelLowering.cpp:19832-19839` (`LowerFLDEXP`)

```cpp
SDValue WideX = widenSubVector(X, true, Subtarget, DAG, DL, 512);
SDValue WideExp = widenSubVector(Exp, true, Subtarget, DAG, DL, 512);
Exp = DAG.getNode(ISD::SINT_TO_FP, DL, WideExp.getSimpleValueType(), Exp);
SDValue Scalef =
    DAG.getNode(X86ISD::SCALEF, DL, WideX.getValueType(), WideX, WideExp);
```

The SINT_TO_FP node is built from the *non-widened* `Exp` into the widened
result VT (element-count mismatch, ill-formed), and then it is dropped on the
floor: SCALEF takes `WideExp` (the raw integer subvector widened to 512 bits),
not the converted FP value. The bug is two-fold:

1. The intended chain is `widen(Exp) -> SINT_TO_FP -> SCALEF`; instead it is
   `Exp -> SINT_TO_FP` (dead) and `widen(Exp) -> SCALEF`.
2. SCALEF expects two FP vector operands; passing an integer vector causes its
   raw bits to be interpreted as IEEE float biases. So an exponent of `1`
   (bit pattern 0x00000001) is interpreted by VSCALEFPS as a denormal float
   ~1.4e-45, and the result is `x * 2^0 == x` rather than `x * 2^1 == 2x`.

The bug is reached on any AVX512F target without VLX for vector fldexp with
operand width < 512 bits (e.g., `<4 x float>`, `<2 x double>`, `<8 x float>`,
`<4 x double>`, and the v8f16/v16f16 fall-through path when `hasFP16()` is
false and we extend to f32).

### Candidate IR

```
define <4 x float> @t(<4 x float> %x, <4 x i32> %exp) {
  %r = call <4 x float> @llvm.ldexp.v4f32.v4i32(<4 x float> %x, <4 x i32> %exp)
  ret <4 x float> %r
}
declare <4 x float> @llvm.ldexp.v4f32.v4i32(<4 x float>, <4 x i32>)
```

### Observed (wrong) output

`llc -mtriple=x86_64-- -mattr=+avx512f`:

```
vmovaps %xmm1, %xmm1
vmovaps %xmm0, %xmm0
vscalefps %zmm1, %zmm0, %zmm0     ; <-- xmm1 still holds INTEGER bits
```

Compare with `-mattr=+avx512f,+avx512vl` which inserts the required
`vcvtdq2ps %xmm1, %xmm1` before the `vscalefps`. The non-VLX path is missing
that conversion.

### Expected wrong outcome

For `ldexp(<1.0, 2.0, 4.0, 8.0>, <1, 2, 3, 4>)`, the correct result is
`<2.0, 8.0, 32.0, 128.0>`. The buggy lowering computes
`x * 2^denorm(int_bits)` which is essentially `x * 1` (with denormal
flush-to-zero) or `x * tiny` (without). E.g. `1.0 * 2^bitcast<f32>(1)` is
`1.0 * 1.4e-45` instead of `1.0 * 2 = 2.0`. Easy fuzzer differential vs the
VLX or scalar path.

### Cross-reference

`llvm/test/CodeGen/X86/ldexp-avx512.ll` does exercise this path (RUN line
`-mattr=+avx512f`) — and its CHECK lines DOCUMENT the bug. In
`test_ldexp_4xfloat`, `test_ldexp_2xdouble`, `test_ldexp_8xfloat`, etc., the
AVX512 (non-VLX) CHECK pattern is:

```
; AVX512:       vmovaps %xmm1, %xmm1
; AVX512:       vmovaps %xmm0, %xmm0
; AVX512:       vscalefps %zmm1, %zmm0, %zmm0
```

with NO `vcvtdq2ps` before `vscalefps`, while the AVX512VL CHECK on the same
function correctly contains `vcvtdq2ps %xmm1, %xmm1` first. The buggy CHECK
lines were obviously auto-generated and accepted. A differential execution
test (compare AVX512F vs AVX512VL output, or vs the scalar fallback) would
flag this immediately.

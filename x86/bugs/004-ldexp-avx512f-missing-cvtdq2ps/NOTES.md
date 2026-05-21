# 004 — `llvm.ldexp.v4f32.v4i32` miscompiled on AVX-512F (no VL): missing `vcvtdq2ps`

Component: X86ISelLowering — `LowerFLDEXP`

## Source

`llvm/lib/Target/X86/X86ISelLowering.cpp` :: `LowerFLDEXP`
(around lines 19832–19839 in this build):

```cpp
SDValue WideX   = widenSubVector(X,   true, Subtarget, DAG, DL, 512);
SDValue WideExp = widenSubVector(Exp, true, Subtarget, DAG, DL, 512);
Exp = DAG.getNode(ISD::SINT_TO_FP, DL, WideExp.getSimpleValueType(), Exp);
SDValue Scalef =
    DAG.getNode(X86ISD::SCALEF, DL, WideX.getValueType(), WideX, WideExp);
```

Two bugs in three lines:

1. The `SINT_TO_FP` node is constructed from `Exp` (the **non-widened** value),
   but its result VT is the widened type — type mismatch, and the node is
   *dropped on the floor*.
2. `X86ISD::SCALEF` is then fed `WideExp` — the **raw integer** vector — as
   if it were a float vector. `VSCALEFPS` interprets the integer bits as
   IEEE-754 floats, so an exponent of `1` (bit pattern `0x00000001`, a
   denormal float ≈ `1.4e-45`) becomes a scale of `2^0 = 1`, etc.

The combined effect is essentially `x * 1.0` instead of `x * 2^e`.

The path is reached on any AVX-512F target without VL for sub-512-bit vector
`ldexp` (`<4 x float>`, `<2 x double>`, `<8 x float>`, `<4 x double>`, and the
f16 fall-through extending to f32).

## Runtime demonstration

`repro.ll` is a one-line `llvm.ldexp.v4f32.v4i32` wrapper. `runner.c` calls it
with `x = <1, 2, 4, 8>` and `e = <1, 2, 3, 4>`; the correct result is
`<2, 8, 32, 128>`.

Output from `./cmd.sh` on a real AVX-512 host:

```
lane 0: got 1  expected 2
lane 1: got 2  expected 8
lane 2: got 4  expected 32
lane 3: got 8  expected 128
FAIL — ldexp produced wrong value (LowerFLDEXP non-VLX bug)
```

The buggy asm is exactly:

```
vmovaps %xmm1, %xmm1
vmovaps %xmm0, %xmm0
vscalefps %zmm1, %zmm0, %zmm0   ; <-- xmm1 still holds INTEGER bits
```

vs. the correct VL path:

```
vcvtdq2ps %xmm1, %xmm1
vscalefps %xmm1, %xmm0, %xmm0
```

## Why it survived testing

`llvm/test/CodeGen/X86/ldexp-avx512.ll` exercises this exact path (run line
`-mattr=+avx512f`). The auto-generated `AVX512:` `CHECK` lines lock in the
buggy three-instruction sequence (no `vcvtdq2ps`), so the test suite asserts
that the bug is preserved.

## Fix

Use `WideExp` as the source of the `SINT_TO_FP` (so it actually converts the
widened integer vector to FP) and pass the *result* to SCALEF:

```cpp
SDValue WideX     = widenSubVector(X,   true, Subtarget, DAG, DL, 512);
SDValue WideExp   = widenSubVector(Exp, true, Subtarget, DAG, DL, 512);
SDValue WideExpFP = DAG.getNode(ISD::SINT_TO_FP, DL,
                                WideX.getValueType(),  // <-- match WideX's FP type
                                WideExp);
SDValue Scalef    = DAG.getNode(X86ISD::SCALEF, DL, WideX.getValueType(),
                                WideX, WideExpFP);
```

## Files
- `repro.ll`  — IR
- `runner.c`  — driver
- `cmd.sh`    — shows both asm forms and runs the buggy version

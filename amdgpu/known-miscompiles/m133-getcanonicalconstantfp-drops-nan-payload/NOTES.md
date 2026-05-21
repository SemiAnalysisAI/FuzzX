# m133: `getCanonicalConstantFP` drops SNaN and non-default-QNaN payload, returning the default-payload canonical QNaN

*Discovery method: code inspection.*  The bug is acknowledged by an
in-source FIXME at line 15841.  Sibling shape to m115/m124 (other
fcanonicalize bugs) and m118 (target intrinsics over-promised as
canonical).

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:15820-15854`
(`getCanonicalConstantFP`):

```cpp
SDValue SITargetLowering::getCanonicalConstantFP(SelectionDAG &DAG, ...) const {
  ...
  // F.10.2.6 / IEEE-2008 fcanonicalize: turn SNaN into QNaN.
  if (C.isSignaling()) {
    // FIXME: Is this supposed to preserve payload bits?
    return DAG.getConstantFP(CanonicalQNaN, SL, VT);   // <-- drops payload
  }

  ...

  // For QNaN with non-default payload, also normalise to default:
  if (C.bitcastToAPInt() != CanonicalQNaN.bitcastToAPInt())
    return DAG.getConstantFP(CanonicalQNaN, SL, VT);   // <-- drops payload
  ...
}
```

AMDGPU HW `v_max_f32(SNaN, SNaN)` (the instruction `fcanonicalize`
lowers to) quiets by setting bit 22 only and **preserves the rest of
the payload**.  Same for QNaN: HW returns the QNaN unchanged.

The constant-fold returns the canonical QNaN with the default payload
(`0x7FC00000`), losing the original payload bits.

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @snan_payload(ptr addrspace(1) %out) {
  ; SNaN 0x7F8A5A5A, payload 0x0A5A5A.
  %c = call float @llvm.canonicalize.f32(float bitcast (i32 2139502170 to float))
  store float %c, ptr addrspace(1) %out
  ret void
}
```

Codegen with `clang -mcpu=gfx950 -O2`:

```asm
snan_payload:
        v_mov_b32_e32 v0, 0x7fc00000   ; default-payload QNaN
        global_store_dword v0, v0, s[0:1]
```

Compare with `-mllvm -amdgpu-enable-uniform-intrinsic-combine=false`
(or O0): HW `v_max_f32 0x7F8A5A5A, 0x7F8A5A5A` produces `0x7FCA5A5A`
(quiet bit set, payload preserved).

* O0 / direct HW: `0x7FCA5A5A`.
* O2 constant fold: `0x7FC00000`.

Difference in 0x0a5a5a payload bits is observable (caller stores the
bit pattern; runtime sees a different NaN).

QNaN case (`qnan_payload`): input `0x7FCDEF12`, expected
`0x7FCDEF12`, observed `0x7FC00000`.

Same shape applies to v2f16 / v2bf16 vector path at line 15898 (the
fold is symmetric across element types).

## Suggested fix

For SNaN: set bit 22 (the quiet bit) explicitly and preserve the rest
of the payload:

```cpp
if (C.isSignaling()) {
  APInt PayloadAndExp = C.bitcastToAPInt();
  PayloadAndExp.setBit(C.getSemantics().precision - 2);   // quiet bit
  return DAG.getConstantFP(APFloat(C.getSemantics(), PayloadAndExp),
                           SL, VT);
}
```

For QNaN with non-default payload: just return the constant unchanged
(don't fold).

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (payload dropped). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Same FIXME present, same defect. |

## Why the fuzzer hasn't caught it

* The FP emitter rarely seeds non-default-payload NaN constants
  feeding `llvm.canonicalize`.
* The interpreter oracle (and m092-style harness oracles) treat all
  NaN bit-patterns as equivalent.
* Per `MEMORY.md` (Prefer-random-over-idioms), the right hook is to
  weight non-default-payload NaN bit-patterns (e.g. `0x7F8A5A5A`,
  `0x7FCDEF12`, `0x7F800001` SNaN) higher in the f32 constant pool
  and add a bit-exact oracle for fcanonicalize results.

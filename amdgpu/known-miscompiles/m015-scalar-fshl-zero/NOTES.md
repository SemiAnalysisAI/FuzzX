# m015: scalar `fshl` by zero returns zero at `-O0`

## Reproducer

```bash
amdgpu/known-miscompiles/run_ll_reproducer.sh \
  amdgpu/known-miscompiles/m015-scalar-fshl-zero/reduced.ll
```

With the instrumented LLVM build at `amdgpu/build/llvm-fuzzer`, the reduced
testcase produces:

```text
input=0x00000003
O0=0x00000000
O2=0x00000003
mismatch=true
```

ROCm 7.1.1 does not reproduce this mismatch; both optimization levels return
`0x00000003`.

## Root Cause Notes

The original fuzzer finding was
`amdgpu/findings/cxx-diff-1778937697-1007403`. The first divergent value was an
`llvm.fshl.i32` with a constant zero shift amount. At `-O2`, the operation is
simplified to the left input, which is the defined result for
`llvm.fshl.i32(x, y, 0)`.

At `-O0`, AMDGPU scalarizes the left input into SGPRs and lowers the zero-count
`fshl` through a 64-bit shift sequence. The generated scalar sequence computes a
second shift count of `-1`:

```asm
s_lshr_b32 s4, s6, 1
s_mov_b32 s0, 0x5040305
s_mov_b32 s1, s6
s_lshr_b64 s[0:1], s[0:1], 1
s_mov_b32 s1, s4
s_mov_b32 s4, -1
s_lshr_b64 s[0:1], s[0:1], s4
```

That final `s_lshr_b64` by `-1` zeros the value in this reproducer. A direct
VGPR-source `fshl(load, const, 0)` matches, and `fshr(const, scalar, 0)` also
matches, so the suppression is limited to generated `fshl` zero-count shapes.

## Fuzzer Suppression

The directed C++ fuzzer avoids generating zero-count `llvm.fshl.i32` by default.
Set `FUZZX_ALLOW_M015_SCALAR_FSHL_ZERO=1` to re-enable this bug class.

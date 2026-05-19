# m049: vector `fshl` by zero returns zero at `-O0`

## Reproducer

```bash
amdgpu/known-miscompiles/run_ll_reproducer.sh \
  amdgpu/known-miscompiles/m049-vector-fshl-zero/reduced.ll
```

With the patched LLVM HEAD build at `amdgpu/build/llvm-fuzzer`, the reduced
testcase produces:

```text
input=0x00000100
O0=0x00000000
O2=0x00000100
mismatch=true
```

ROCm 7.2.3 does not reproduce this mismatch. Patched LLVM HEAD and patched ROCm
HEAD do reproduce it.

## Root Cause Notes

The original fuzzer finding was
`/tmp/fuzzx-amdgpu-orenamd@semianalysis.com/head-pr198373-196418-198412-198419-m048-20260519-032328/findings/cxx-diff-1779161280-3327865`.
The first divergent value was an oracle mismatch at input index 0:

```text
input=0x0
O0=0x00191491
O2=0x0019b4d5
expected=0x0019b4d5
```

After reduction, the only interesting operation is:

```llvm
%vec = insertelement <4 x i32> zeroinitializer, i32 %x, i32 3
%fshl = call <4 x i32> @llvm.fshl.v4i32(
    <4 x i32> %vec,
    <4 x i32> zeroinitializer,
    <4 x i32> zeroinitializer)
%result = extractelement <4 x i32> %fshl, i32 3
```

For shift amount zero, `llvm.fshl` is defined to return the left operand, so
the expected lane-3 result is the input value. At `-O2`, LLVM simplifies the
operation to a load/store of `%x`. At `-O0`, AMDGPU lowers the vector `fshl`
through a 64-bit shift sequence with a derived shift count of `-1`, and the
stored lane becomes zero.

## Fuzzer Suppression

The directed C++ fuzzer now suppresses vector `llvm.fshl` calls by default. Set
`FUZZX_ALLOW_M049_VECTOR_FSHL=1` to re-enable this pattern. The reduced
testcase uses a zero shift vector, but the original fuzzer input also reproduced
with a nonzero constant shift vector, so the suppression covers the vector
intrinsic family rather than only the reduced zero-count case.

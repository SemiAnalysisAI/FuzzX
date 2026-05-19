# m050: `and x, (x - 0)` feeding `ctpop` is lowered through the wrong `bitop3`

## Reproducer

```bash
amdgpu/known-miscompiles/run_ll_reproducer.sh \
  amdgpu/known-miscompiles/m050-bitcount-and-sub-zero/reduced.ll
```

Before llvm/llvm-project#198373 was applied to the LLVM HEAD build, the reduced
testcase produced:

```text
[0] input=0x00000000 O0=0x0000001f O2=0x7fffffff mismatch=true
any_mismatch=true
```

ROCm 7.2.3 and patched ROCm HEAD do not reproduce this mismatch. Current
patched LLVM HEAD also passes after llvm/llvm-project#198373.

## Root Cause Notes

The original fuzzer finding was
`/tmp/fuzzx-amdgpu-orenamd@semianalysis.com/head-pr198373-196418-198412-198419-m048-20260519-032328/findings/cxx-diff-1779161281-3327710`.
The first divergent value was an oracle mismatch at input index 0:

```text
input=0x0
O0=0x0000001d
O2=0x7ffffffd
expected=0x7ffffffd
```

The reduced testcase keeps one loop iteration so the `%acc` phi remains a real
value at `-O0`. In the body, `%masked` is `0x7fffffff`, `%sel` is zero, and
therefore `%set` is also `0x7fffffff`. The expression:

```llvm
%dec = sub i32 %set, 0
%clear = and i32 %set, %dec
%pop.a = call i32 @llvm.ctpop.i32(i32 %set)
%pop.clear = call i32 @llvm.ctpop.i32(i32 %clear)
%delta = sub i32 %pop.a, %pop.clear
%mix = add i32 %clear, %delta
```

is defined to leave `%mix == %set`, because `%dec == %set` and both `ctpop`
calls count the same value. At `-O2`, the testcase stores `0x7fffffff`.

Without llvm/llvm-project#198373, `-O0` lowers `%clear` through a
`v_bitop3_b32` combine using the pieces that formed `%set`; that combine
produces zero for `%clear`. The later scalar `ctpop` then computes `31 - 0`,
so the stored value becomes `0x0000001f`.

## Fuzzer Follow-Up

The directed C++ fuzzer allows `and X, (sub X, 0)` shapes with the current
patched LLVM HEAD toolchain.

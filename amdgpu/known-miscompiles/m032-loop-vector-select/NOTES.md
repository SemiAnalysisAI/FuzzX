# m032: ROCm 7.2.3 `-O2` kills the loop EXEC mask before store

Found while fuzzing the ROCm 7.2.3 source build with the LLVM-bitcode C++
fuzzer after increasing CFG complexity and minimizing the live corpus. The
original fuzzer input reduced to a one-iteration loop whose loop-carried value
passes through a vector compare/select expression and a dynamic scalar select.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m032-loop-vector-select/reduced.ll
```

Observed result on the ROCm 7.2.3 source build:

```text
input=0x00000000
O0=0x00000001
O2=0x00000000
mismatch=true
```

## Reduction

The reduced testcase is fully defined and uses the normal fuzzer kernel shape:
one guarded work-item stores one `i32` output. For work-item zero, the loop runs
once. The vector path computes a true `%is_small`, so `%z == 1` and
`ctlz(1) == 31`. The final scalar select condition is `%wi == 999`, which is
false for the reproducer lane, so the loop-carried `%result` must be `1`:

```llvm
%cmp = icmp sle <4 x i32> %w0, zeroinitializer
%selv = select <4 x i1> %cmp, <4 x i32> zeroinitializer,
               <4 x i32> <i32 3, i32 0, i32 0, i32 0>
%x = extractelement <4 x i32> %selv, i32 0
%is_small = icmp slt i32 %x, 1
%z = zext i1 %is_small to i32
%lz = call i32 @llvm.ctlz.i32(i32 %z, i1 false)
%result = select i1 %argbool, i32 %lz, i32 1
```

## Root Cause Notes

The ROCm 7.2.3 `-O2` output contains an unconditional EXEC-mask kill after the
entry bounds check:

```asm
v_cmp_gt_u32_e32 vcc, s2, v0
s_and_saveexec_b64 s[2:3], vcc
s_cbranch_execz ...
s_mov_b64 s[2:3], 0
s_and_b64 exec, exec, s[2:3]
s_cbranch_execz ...
```

That makes the later `global_store_dword` unreachable for active lanes, so the
output buffer keeps its initialized zero. The `-O0` code stores `1`, which is
the defined result of the IR.

This points at an optimized CFG/loop transform incorrectly proving the loop
body inactive or replacing its active-lane mask with zero. The vector
compare/select feeding the loop-carried scalar value appears to be the trigger:
replacing the dynamic false scalar select with a constant false select, or
removing the vector-select-derived value, makes the reduced testcase pass.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces: `O0=0x00000001`, `O2=0x00000000`. |
| LLVM HEAD, commit `10756d32f96154f0889eda159ea9a26bc4188bda` | Passes: `O0=0x00000001`, `O2=0x00000001`. |
| ROCm HEAD, commit `9115c466b3577830455f70c4f492429bf6c64b25` | Passes: `O0=0x00000001`, `O2=0x00000001`. |

Original fuzzer input SHA-1:

```text
7daf8044e12e0f1858d2030bbff328a41c5cf33c
```

## Fuzzer Follow-Up

The IR-bitcode fuzzer now suppresses loop-carried accumulator values whose
backedge depends on a vector `select`. Set
`FUZZX_ALLOW_M032_LOOP_VECTOR_SELECT=1` to re-enable this shape when replaying
the original fuzzer input.

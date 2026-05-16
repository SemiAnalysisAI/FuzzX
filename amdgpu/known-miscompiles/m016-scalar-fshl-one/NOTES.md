# m016: scalar `fshl` by one returns only the carry bit at `-O0`

## Reproducer

```bash
amdgpu/known-miscompiles/run_ll_reproducer.sh \
  amdgpu/known-miscompiles/m016-scalar-fshl-one/reduced.ll
```

With the instrumented LLVM build at `amdgpu/build/llvm-fuzzer`, the reduced
testcase produces:

```text
input=0xf2f2f2fc
O0=0x00000001
O2=0xe5e5e5f9
mismatch=true
```

ROCm 7.1.1 does not reproduce this mismatch; both optimization levels return
`0xe5e5e5f9`.

## Root Cause Notes

The original fuzzer finding was
`amdgpu/findings/cxx-diff-1778939210-1018720`. SSA checkpointing showed the
first divergent value at:

```llvm
%34 = call i32 @llvm.fshl.i32(i32 %33, i32 -218959118, i32 1)
```

For the minimized input, the defined result is
`(0xf2f2f2fc << 1) | (0xf2f2f2f2 >> 31) = 0xe5e5e5f9`.

At `-O0`, AMDGPU scalarizes the left input through `v_readfirstlane_b32` and
lowers the one-count `fshl` as a 64-bit logical shift by `-1`:

```asm
v_readfirstlane_b32 s4, v2
s_mov_b32 s0, 0xf2f2f2f2
s_mov_b32 s1, s4
s_mov_b32 s4, -1
s_lshr_b64 s[0:1], s[0:1], s4
```

That leaves only bit 31 of the scalar input in the low word, so this testcase
returns `1`. This is the same scalar `fshl` lowering family as m015, but it
also affects a nonzero shift count.

## Fuzzer Suppression

The directed C++ fuzzer avoids generating `llvm.fshl.i32` by default after this
finding. Set `FUZZX_ALLOW_M016_SCALAR_FSHL=1` to re-enable nonzero scalar
`fshl` coverage. Setting `FUZZX_ALLOW_M015_SCALAR_FSHL_ZERO=1` also re-enables
`fshl` generation so the zero-count m015 shape can be reproduced.

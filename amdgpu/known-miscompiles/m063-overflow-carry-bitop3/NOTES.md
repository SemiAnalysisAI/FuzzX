# m063: overflow-derived carry expression is miscompiled through `v_bitop3_b32` at `-O0`

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373,
llvm/llvm-project#196418, llvm/llvm-project#198412, and
llvm/llvm-project#198419 applied. The original oracle finding was:

```text
kind=oracle
index=0
input=0x0
o0=0x6
o2=0x2
expected=0x2
```

Run the reproducer with:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m063-overflow-carry-bitop3/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches:

```text
input=0x00000000
O0=0x00000006
O2=0x00000002
mismatch=true
```

## Reduction

The reduced kernel keeps a small overflow-derived carry/majority expression:

```llvm
%ov = call { i32, i1 } @llvm.umul.with.overflow.i32(i32 %v, i32 %wi)
%x = xor i32 %value, %overflow.i32
%ab = and i32 %x, %x
%ac = and i32 %x, 2
%maj0 = or i32 %ab, %ac
%majority = or i32 %maj0, %ac
%result = xor i32 2, (shl i32 %majority, 1)
```

For lane 0 with input `0`, `%x` is `0`, so the correct result is `2`.

## Root Cause Notes

The reduced `-O0` assembly computes the multiply-with-overflow value and then
lowers the duplicated carry/majority expression through `v_bitop3_b32`:

```asm
v_mad_u64_u32 v[4:5], s[2:3], v2, v3, 0
v_cmp_ne_u32_e64 s[2:3], v5, s1
v_cndmask_b32_e64 v4, 0, 1, s[2:3]
v_xor_b32_e64 v2, v4, v4
v_xor_b32_e64 v2, v2, s0
v_bitop3_b32 v3, v3, s0, v4 bitop3:0xfc
v_lshlrev_b32_e64 v3, s0, v3
v_xor_b32_e64 v2, v2, v3
```

That sequence stores `6`. The optimized path computes `%x`, shifts it, and
xors with `2`, storing the interpreter/oracle result `2`. This points to an
`-O0` `v_bitop3_b32` lowering issue for the duplicated carry expression, not an
IR semantics issue.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Passes: `O0=0x00000002`, `O2=0x00000002`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: `O0=0x00000006`, `O2=0x00000002`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: `O0=0x00000006`, `O2=0x00000002`. |

Original fuzzer input SHA-1:

```text
698ec726352b90fc6a638f6b916f0f25c1e58cc5
```

Reduced reproducer SHA-1:

```text
8765d59033191b6f062f98bb2399f0857fe36354
```

## Fuzzer Follow-Up

The fuzzer now rejects final stores depending on generated `carry` idiom values
by default. Set `FUZZX_ALLOW_M063_OVERFLOW_CARRY_BITOP3=1` to re-enable this
bug class.

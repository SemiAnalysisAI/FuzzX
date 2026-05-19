# m044: `<4 x i32>` self-and loses lane before zero shuffle OR

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373,
llvm/llvm-project#196418, llvm/llvm-project#198412, and
llvm/llvm-project#198419 applied. The original oracle finding was:

```text
input=0x00000000
O0=0x000000e1
O2=0x000000df
expected=0x000000df
```

The reduced reproducer keeps the vector identity and zero-shuffle shape:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m044-v4i32-self-and-zero-shuffle/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches:

```text
input=0x00000000
O0=0x00000000
O2=0x00000001
mismatch=true
```

## Reduction

For input zero:

```llvm
%masked = and i32 %x, 1       ; 0
%a = or i32 %masked, 1        ; 1
%v = insertelement <4 x i32> zeroinitializer, i32 %a, i32 0
%and = and <4 x i32> %v, %v   ; %v
```

`%zero.insert` is still the zero vector, and `%zero.shuffle` is therefore also
zero. The extracted lane of `%or` must be one.

## Root Cause Notes

At `-O0`, LLVM HEAD lowers the reduced expression through:

```asm
s_mov_b32 s3, 1
v_mov_b32_e32 v1, s8
v_mov_b32_e32 v2, s3
v_bitop3_b32 v2, s2, v1, v2 bitop3:0xec
v_mov_b32_e32 v1, v2
global_store_dword v0, v1, s[0:1]
```

At the `v_bitop3_b32`, `s2` is the loaded input, `v1` is zero, and `v2` is one.
For input zero this computes and stores zero, dropping the forced-one value from
the `<4 x i32>` self-`and` lane. The `-O2` pipeline folds the identity and
stores `1`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Passes: `O0=0x00000001`, `O2=0x00000001`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: `O0=0x00000000`, `O2=0x00000001`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Passes: `O0=0x00000001`, `O2=0x00000001`. |

## Fuzzer Follow-Up

The fuzzer now rejects `<4 x i32>` vector identity `and` shapes by default. Set
`FUZZX_ALLOW_M044_V4I32_SELF_AND=1` to re-enable this bug class.

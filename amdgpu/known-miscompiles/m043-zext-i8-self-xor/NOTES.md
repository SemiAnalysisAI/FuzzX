# m043: self-xor after `zext i8` lowers to nonzero `v_bitop3_b32`

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373,
llvm/llvm-project#196418, llvm/llvm-project#198412, and
llvm/llvm-project#198419 applied. The original fuzzer program contained
byte-pack and loop scaffolding; the reduced testcase keeps only the scalar
identity expression that triggers the bad `-O0` lowering.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m043-zext-i8-self-xor/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches:

```text
input=0x00000000
O0=0x00000001
O2=0x00000000
mismatch=true
```

## Reduction

For the single work-item, `%wi == 0`, so:

```llvm
%tr = trunc i32 %wi to i8 ; 0
%z = zext i8 %tr to i32  ; 0
%x = xor i32 %z, 1       ; 1
%r = xor i32 %x, %x      ; 0
```

The defined result is therefore zero.

## Root Cause Notes

At `-O0`, LLVM HEAD combines the expression into:

```asm
v_and_b32_e64 v0, v0, s0
v_mov_b32_e32 v1, s1
v_bitop3_b32 v2, v0, s0, v1 bitop3:0x9c
```

where `s0 == 1` and `s1 == 0xff`. For `%wi == 0`, this produces `1` even
though both IR operands of the final `xor` are the same SSA value. The `-O2`
pipeline folds the self-xor to zero before AMDGPU lowering.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Passes: `O0=0x00000000`, `O2=0x00000000`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: `O0=0x00000001`, `O2=0x00000000`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Passes: `O0=0x00000000`, `O2=0x00000000`. |

## Fuzzer Follow-Up

The fuzzer now rejects scalar `xor x, x` by default. Set
`FUZZX_ALLOW_M043_SELF_XOR=1` to re-enable this bug class.

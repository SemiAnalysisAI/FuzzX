# m042: `or x, (lshr x, 0)` mislowers xor-smear through `v_bitop3_b32`

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373,
llvm/llvm-project#196418, and llvm/llvm-project#198412 applied. The original
fuzzer program was a larger bit-count/vector-reduction expression. The reduced
testcase keeps the scalar expression that triggers the bad `-O0` lowering.

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m042-or-lshr-zero-xor/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches:

```text
[10] input=0xafa72a31 O0=0xefefffbf O2=0xc1cfff8f mismatch=true
```

## Reduction

For lane 10, `%v == 0xafa72a31` and `%salt == 10 * 0x9e3779b9 ==
0x2e2ac13a`, so:

```text
mix = v ^ salt = 0x818deb0b
shr = mix >> 1 = 0x40c6f585
expected = mix | shr = 0xc1cfff8f
```

The final `lshr i32 %smear1, 0` is an identity, so the second `or` should keep
`%smear1` unchanged.

## Root Cause Notes

At `-O0`, LLVM HEAD lowers the first smear through:

```asm
v_xor_b32_e64 v3, v2, v4
v_lshrrev_b32_e64 v3, s2, v3
v_bitop3_b32 v2, v2, v3, v4 bitop3:0xfe
```

`bitop3:0xfe` is three-input OR, so this computes
`v | salt | ((v ^ salt) >> 1)`, giving `0xefefffbf` for lane 10. The intended
truth table must preserve the `(v ^ salt)` input. The `-O2` lowering uses the
correct `bitop3:0xf6` form and returns `0xc1cfff8f`.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Passes: `O0=0xc1cfff8f`, `O2=0xc1cfff8f`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, and llvm/llvm-project#198412 applied locally | Reproduces: `O0=0xefefffbf`, `O2=0xc1cfff8f`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, and llvm/llvm-project#198412 applied locally | Passes: `O0=0xc1cfff8f`, `O2=0xc1cfff8f`. |

## Fuzzer Follow-Up

The fuzzer now rejects redundant `or x, (lshr x, 0)` shapes by default. Set
`FUZZX_ALLOW_M042_OR_LSHR_ZERO=1` to re-enable this bug class.

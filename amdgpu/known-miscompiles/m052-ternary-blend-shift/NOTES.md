# m052: ternary blend feeding a shift mask drops one operand

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373,
llvm/llvm-project#196418, llvm/llvm-project#198412, and
llvm/llvm-project#198419 applied. The original oracle finding was:

```text
kind=oracle
index=2
input=0x7FFFFFFF
o0=0x2
o2=0x80000000
expected=0x80000000
```

The minimized reproducer keeps the same scalar idiom but forces only lane 4 to
run it:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m052-ternary-blend-shift/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches:

```text
input=0xffffffff
O0=0x40000000
O2=0x910c8fc0
mismatch=true
```

ROCm 7.2.3 passes this reduced testcase. LLVM HEAD and ROCm HEAD reproduce the
mismatch.

## Reduction

The reduced program computes:

```llvm
%mask = xor i32 %acc, %wi
%not = xor i32 %mask, -1
%right = and i32 %wi, %not
%blend = or i32 %mask, %right
%shift = and i32 %blend, 31
```

Because `%mask == %acc ^ %wi`, `%blend` is equivalent to `%acc | %wi`. The
result then feeds a manually expanded funnel-shift-like expression with a masked
shift count. Replacing `%blend` with `or i32 %acc, %wi` makes `-O0` and `-O2`
agree, so the mismatch comes from lowering this ternary blend shape.

## Root Cause Notes

At `-O0`, LLVM HEAD lowers the blend and shift-mask sequence through:

```asm
v_xor_b32_e64 v3, v2, v1
v_not_b32_e32 v0, v3
v_and_or_b32 v0, v1, v0, v3
v_bitop3_b32 v3, v2, s1, v1 bitop3:0xc0
```

The `v_and_or_b32` computes the intended `%blend`, but the following
`v_bitop3_b32` computes `%acc & 31` for `%shift`. It should compute
`(%acc | %wi) & 31`, preserving the `%wi` low bits. The `-O2` lowering uses the
equivalent `or` first and then masks the OR result, producing the oracle value.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Passes: `O0=0x910c8fc0`, `O2=0x910c8fc0`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: `O0=0x40000000`, `O2=0x910c8fc0`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: `O0=0x40000000`, `O2=0x910c8fc0`. |

## Fuzzer Follow-Up

The fuzzer now rejects ternary blend shapes of the form
`((a ^ b) | (b & ~(a ^ b))) & 31` by default. Set
`FUZZX_ALLOW_M052_TERNARY_BLEND_SHIFT=1` to re-enable this bug class.

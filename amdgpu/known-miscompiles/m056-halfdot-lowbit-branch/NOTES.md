# m056: halfword-dot low-bit branch is miscompiled at `-O0`

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373,
llvm/llvm-project#196418, llvm/llvm-project#198412, and
llvm/llvm-project#198419 applied. The original oracle finding was:

```text
kind=oracle
index=2
input=0x7FFFFFFF
o0=0x0
o2=0xFFFD7FFC
expected=0xFFFD7FFC
```

Run the reproducer with:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m056-halfdot-lowbit-branch/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches:

```text
[2] input=0x7fffffff O0=0x00000000 O2=0xfffd7ffc mismatch=true
```

## Reduction

The reduced kernel uses a full 256-lane input list because the reducer removed
the original `idx < n` guard. The live lane computes a halfword multiply and
byte-pack expression, masks the low two bits, and branches between storing zero
or storing the packed value plus the input.

For lane 2, the LLVM interpreter and `-O2` agree on `0xfffd7ffc`, while LLVM
HEAD `-O0` takes the zero-store path.

## Root Cause Notes

The reduced `-O0` assembly computes the low-bit branch key with a `v_bitop3_b32`
sequence over the packed halfword-dot value:

```asm
v_xor_b32_e64 v2, v0, v1
v_bitop3_b32 v0, v0, s0, v1 bitop3:0x48
v_cmp_ne_u32_e64 s[0:1], v0, s0
```

That mask sends lane 2 to the zero-store block. The `-O2` lowering computes the
same source expression with a different `v_perm_b32` / `v_and_or_b32` sequence
and stores the oracle value. This looks like another low-bit `v_bitop3_b32`
lowering issue exposed by the generated halfword-dot pack, not an IR semantics
issue.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Passes: lane 2 `O0=0xfffd7ffc`, `O2=0xfffd7ffc`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: lane 2 `O0=0x00000000`, `O2=0xfffd7ffc`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: lane 2 `O0=0x00000000`, `O2=0xfffd7ffc`. |

## Fuzzer Follow-Up

The fuzzer now rejects low-bit branch keys depending on generated halfword-dot
pack values by default. Set `FUZZX_ALLOW_M056_HALFDOT_BRANCH=1` to re-enable
this bug class.

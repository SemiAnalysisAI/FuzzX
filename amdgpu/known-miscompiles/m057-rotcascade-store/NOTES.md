# m057: rotate cascade final store is miscompiled at `-O0`

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373,
llvm/llvm-project#196418, llvm/llvm-project#198412, and
llvm/llvm-project#198419 applied. The original oracle finding was:

```text
kind=oracle
index=1
input=0x1
o0=0xBA6CFC29
o2=0xDBAF277C
expected=0xDBAF277C
```

Run the reproducer with:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m057-rotcascade-store/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches:

```text
[1] input=0x00000001 O0=0xba6cfc29 O2=0xdbaf277c mismatch=true
```

## Reduction

The reduced kernel uses a full 256-lane input list because the reducer removed
the original `idx < n` guard. The surviving expression is a repeated rotate /
population-count cascade. Each stage masks the rotate count, builds a rotate
from shifts, counts the rotated value, conditionally merges, and carries the
stage result into the next round.

For lane 1, the LLVM interpreter and `-O2` agree on `0xdbaf277c`, while LLVM
HEAD `-O0` stores `0xba6cfc29`.

## Root Cause Notes

LLVM HEAD `-O0` expands the rotate cascade into a long sequence of VALU shifts,
`v_bcnt_u32_b32`, `v_cndmask_b32`, and `v_bitop3_b32` operations before the
final store. The same source IR at `-O2` is canonicalized into shorter
`v_alignbit_b32` rotate sequences and returns the oracle value.

The reduced case passes both the ROCm 7.2.3 release compiler and the current
ROCm HEAD compiler with the local PR patches, so this appears to be an
LLVM-HEAD-only `-O0` rotate/bitselect lowering regression.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Passes: lane 1 `O0=0xdbaf277c`, `O2=0xdbaf277c`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: lane 1 `O0=0xba6cfc29`, `O2=0xdbaf277c`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Passes: lane 1 `O0=0xdbaf277c`, `O2=0xdbaf277c`. |

## Fuzzer Follow-Up

The fuzzer now rejects final stores depending on generated rotate-cascade
values by default. Set `FUZZX_ALLOW_M057_ROTCASCADE_STORE=1` to re-enable this
bug class.

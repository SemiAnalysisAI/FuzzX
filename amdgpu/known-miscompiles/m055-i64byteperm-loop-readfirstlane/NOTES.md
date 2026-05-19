# m055: i64 byte-permute loop value is miscompiled at `-O0`

Found while fuzzing upstream LLVM HEAD with llvm/llvm-project#198373,
llvm/llvm-project#196418, llvm/llvm-project#198412, and
llvm/llvm-project#198419 applied. The original oracle finding was:

```text
kind=oracle
index=0
input=0x0
o0=0xFFFFFFFF
o2=0xFF22DD00
expected=0xFF22DD00
```

Run the reproducer with:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m055-i64byteperm-loop-readfirstlane/reduced.ll
```

Observed result on LLVM HEAD with the local PR patches:

```text
input=0x00000000
O0=0xffffffff
O2=0xff22dd00
mismatch=true
```

## Reduction

The minimized kernel keeps the normal `idx < n` guard so the one-element
runner does not execute out-of-bounds workitems. `llvm-reduce` got the original
680-line generated program down to 437 lines while preserving that guard and
the exact output mismatch; further instruction-level reduction stalled on
GPU-running candidates.

The remaining shape has a loop-carried `i32` value whose backedge depends on a
branch between a SWAR-style value and an `i64` byte-permutation fold:

```llvm
%wide = or i64 ...
%ctpop = call i64 @llvm.ctpop.i64(i64 %wide)
%add.pop = add i64 %wide, shl i64 %ctpop, 8
%hi = trunc i64 (lshr i64 %add.pop, 32) to i32
%lo = trunc i64 %add.pop to i32
%fold = xor i32 %hi, %lo
%loop.phi = phi i32 [ %fold, %then ], [ %swar, %else ]
```

For input `0`, the LLVM interpreter and `-O2` agree on `0xff22dd00`, while
LLVM HEAD `-O0` returns `0xffffffff`.

## Root Cause Notes

LLVM HEAD `-O0` emits a long divergent loop sequence with `s_or_saveexec_b64`,
`v_readfirstlane_b32`, VALU bit operations, and a final `global_store_dword`.
The bad assembly differs from ROCm HEAD in the local lowering around the
overflow/bitselect part of the loop body:

```asm
v_cndmask_b32_e64 v5, 0, -1, s[0:1]
v_and_b32_e64 v1, v0, v5
v_bitop3_b32 v4, v3, v5, v3 bitop3:0x30
v_bfi_b32 v2, v2, v6, v7
v_bitop3_b32 v0, v0, v3, v5 bitop3:0xf4
```

ROCm HEAD compiles the same region differently and produces the expected
result. This looks like an LLVM HEAD-only `-O0` lowering regression in the
divergent loop/bitselect sequence rather than a source-IR semantics issue.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Passes: `O0=0xff22dd00`, `O2=0xff22dd00`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Reproduces: `O0=0xffffffff`, `O2=0xff22dd00`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Passes: `O0=0xff22dd00`, `O2=0xff22dd00`. |

## Fuzzer Follow-Up

The fuzzer now rejects loop-carried values depending on generated i64
byte-permutation idioms by default. Set
`FUZZX_ALLOW_M055_I64BYTEPERM_LOOP=1` to re-enable this bug class.

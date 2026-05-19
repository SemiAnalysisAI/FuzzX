# m059: `srem` loop branch skips a live lane at `-O0`

Found while fuzzing an upstream LLVM HEAD build that was missing the source fix
from llvm/llvm-project#198373. The original oracle finding was:

```text
kind=oracle
index=1
input=0x1
o0=0x2BD34A38
o2=0x2BD34A25
expected=0x2BD34A25
```

Run the reproducer with:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m059-srem-loop-branch/reduced.ll
```

Observed result before llvm/llvm-project#198373 was applied:

```text
[0] input=0x00000000 O0=0x00000001 O2=0x00000001 mismatch=false
[1] input=0x00000001 O0=0x00000000 O2=0x00000001 mismatch=true
any_mismatch=true
```

## Reduction

The reduced testcase uses two launched workitems. Workitem 1 computes a branch
key from a loop-carried value:

```llvm
%fuzz.cfg.srem.op = srem i32 %fuzz.cfg.srem.num.mask, 35
%fuzz.loop.multi.exit.key = and i32 %fuzz.cfg.srem.op, 1
switch i32 %fuzz.loop.multi.exit.key, label %fuzz.loop.multi.header [
  i32 0, label %common.ret
  i32 1, label %fuzz.loop.multi.break.b
]
```

For workitem 1, the reduced expression is defined and the branch key should be
one: `%fuzz.cfg.srem.num.mask == 0x004d0000`, and
`0x004d0000 srem 35 == 7`. Therefore the kernel should take the `i32 1` case
and store `1` to `out[1]`.

Removing the loop-shaped default backedge makes the reduced testcase pass, so
the surviving reproducer keeps the multi-exit loop structure even though the
launched lanes do not dynamically need to iterate.

## Root Cause Notes

Before llvm/llvm-project#198373 was applied, the `-O0` lowering expanded the
signed remainder through a float reciprocal sequence and then used the low bit
to form an EXEC mask for the switch case. In that stale build, lane 1 did not
reach the store even though the source branch key is one. The relevant tail was:

```asm
v_and_b32_e64 v0, v0, s0
v_cmp_eq_u32_e64 s[2:3], v0, s0
s_and_b64 s[0:1], s[0:1], s[2:3]
s_mov_b64 exec, s[0:1]
s_cbranch_execz .LBB0_4
...
global_store_dword v[0:1], v2, off
```

At `-O2`, LLVM rewrote the branch key to a compact integer multiply-high
sequence and stored `1` for lane 1. Retesting after applying
llvm/llvm-project#198373 makes both `-O0` and `-O2` store `1`, so this reducer
is captured by the already-applied `BitOp3_Op` fix rather than a new `srem`
lowering issue.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Passes: lane 1 `O0=0x00000001`, `O2=0x00000001`. |
| LLVM HEAD, commit `0dd29960cd6102b37651cc3f58f872652099b83b`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Passes: lane 1 `O0=0x00000001`, `O2=0x00000001`. |
| ROCm HEAD, commit `a5de13684ba84db953b28e632ea304080a4318d0`, with llvm/llvm-project#198373, llvm/llvm-project#196418, llvm/llvm-project#198412, and llvm/llvm-project#198419 applied locally | Passes: lane 1 `O0=0x00000001`, `O2=0x00000001`. |

## Fuzzer Follow-Up

The fuzzer allows multi-exit loop branch keys derived from signed remainder
operations with the current patched LLVM HEAD toolchain.

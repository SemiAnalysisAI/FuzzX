# 240 — `X86FrameLowering::emitStackProbeInlineGenericBlock` skips the probe for a one-page alloca, defeating `probe-stack="inline-asm"`

Component: `llvm/lib/Target/X86/X86FrameLowering.cpp` lines ~729, 752

For an alloca of exactly `StackProbeSize` (4096) bytes:
- Line 729 initial-probe guard: `if (StackProbeSize < Offset + AlignOffset)` is `4096 < 4096` → false → no initial probe.
- Line 752 loop guard: `while (CurrentOffset + StackProbeSize < Offset)` is `0 + 4096 < 4096` → false → loop doesn't run.
- Tail dispatch: emits `subq $4096, %rsp` with no probe.

Result: zero probes emitted for a full-page alloca. The OS guard page below the new SP is never touched, defeating the whole point of `probe-stack="inline-asm"`.

Same root family causes off-by-one tail skip for any `Offset` that's an exact multiple of `StackProbeSize` (16384 → 3 sub+probe pairs + 1 unprobed subq).

## Reproducer

`llc -O2 -mtriple=x86_64-unknown-linux-gnu repro.ll -o -`

The emitted prologue has `subq $4096, %rsp` but no `movq $0, (%rsp)` or `or` probe touch.

## Severity

Default x86 -O2 when source uses `__attribute__((no_stack_check_inline))` or builds with `-fstack-clash-protection`. Defeats stack-clash mitigation entirely for unlucky-sized stacks.

## Fix

Change line 729 to `if (StackProbeSize <= Offset + AlignOffset)` (or add an unconditional initial probe), and use `<=` in the loop guard at 752.

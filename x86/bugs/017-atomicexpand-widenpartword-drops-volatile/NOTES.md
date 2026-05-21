# 017 — `AtomicExpandImpl::widenPartwordAtomicRMW` drops the `volatile` flag

Component: CodeGen/AtomicExpandPass

## Source

`llvm/lib/CodeGen/AtomicExpandPass.cpp:1117-1151` (`widenPartwordAtomicRMW`)

This helper widens a sub-word `atomicrmw and/or/xor` (e.g. i8/i16) to the
target's minimum supported word size by adjusting the value operand with a
shift/mask, then constructing a fresh RMW via `IRBuilder::CreateAtomicRMW`.
`CreateAtomicRMW` has **no `IsVolatile` parameter**, and the calling code
never calls `setVolatile(AI->isVolatile())` on the new instruction.

Every other expansion path in this file consistently calls
`setVolatile(...)` after constructing the replacement (see lines 566, 597,
708, 1250, 1433, 1852). The cmpxchg-loop path passes `AI->isVolatile()` into
`insertRMWCmpXchgLoop` at line 1102. Only the widening path silently strips
the `volatile` bit.

Affects targets whose `MinCmpXchgSizeInBits` exceeds the value width:
RISC-V (without Zabha), LoongArch (base), SPARC, AMDGPU, Hexagon, VE,
Xtensa. x86 doesn't directly hit this widening path (x86 supports sub-word
RMWs natively), but the bug lives in the generic pipeline that x86 shares.

## Fix

Add `NewAI->setVolatile(AI->isVolatile());` after the
`Builder.CreateAtomicRMW(...)` call on line 1141.

## Repro

```
; opt -mtriple=riscv32 -atomic-expand -S
define i8 @f(ptr %p, i8 %v) {
  %r = atomicrmw volatile and ptr %p, i8 15 seq_cst, align 1
  ret i8 %r
}
```

Expected: widened i32 RMW retains `volatile`.
Observed: `volatile` is gone in the widened RMW — subsequent passes (DSE,
LICM, GVN) are free to eliminate / hoist / merge the user's volatile access.

Source-confirmed.

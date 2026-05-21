file: llvm/lib/CodeGen/AtomicExpandPass.cpp:1117-1151

`AtomicExpandImpl::widenPartwordAtomicRMW` widens a sub-word
`atomicrmw and/or/xor` (e.g. i8 or i16) into the target's minimum
supported word size (commonly i32) by adjusting the value operand
with a precomputed shift/mask, then constructing a fresh RMW with
`IRBuilder::CreateAtomicRMW`. That helper has no `IsVolatile`
parameter (see include/llvm/IR/IRBuilder.h:1982-1994) and the calling
code never calls `setVolatile(AI->isVolatile())` on the new
instruction, nor does it `copyMetadataForAtomic` propagate that bit
(volatility is a flag on AtomicRMWInst, not metadata). Every other
expansion path in this file consistently calls `setVolatile(...)`
after constructing the replacement (see lines 566, 597, 708, 1250,
1433, 1852 etc.), and `expandPartwordAtomicRMW`'s cmpxchg-loop path
passes `AI->isVolatile()` into `insertRMWCmpXchgLoop` at line 1102.
The widening path is the sole outlier and silently strips `volatile`
from `volatile atomicrmw {and,or,xor} ...` whenever the target's
`MinCmpXchgSizeInBits` exceeds the value width (RISC-V without
Zabha, LoongArch base, Sparc, AMDGPU, Hexagon, VE, Xtensa). For
MMIO-style code that uses idempotent or partword volatile RMWs, this
is a silent wrong-code bug: the resulting RMW is no longer marked
volatile so subsequent passes (DSE, LICM, GVN) may eliminate, hoist,
or merge what the user wrote as a single volatile access.

Repro:

  ; RUN: opt -S -mtriple=riscv32 -atomic-expand < %s
  define i8 @f(ptr %p, i8 %v) {
    %r = atomicrmw volatile and ptr %p, i8 15 seq_cst, align 1
    ret i8 %r
  }

Expected: the widened i32 atomicrmw is also `volatile`.
Observed (likely): a non-volatile i32 atomicrmw — `volatile` is
gone from the resulting `atomicrmw and i32* %AlignedAddr, ...`.

Fix: after the `Builder.CreateAtomicRMW(...)` call on line 1141,
add `NewAI->setVolatile(AI->isVolatile());` (mirroring lines
597, 1250, etc.).

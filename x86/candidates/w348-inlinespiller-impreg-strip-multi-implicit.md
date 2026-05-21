# w348: InlineSpiller::foldMemoryOperand only strips ONE implicit operand even when source MI had several

## Status
SUSPECTED. Stale implicit operands on the folded instruction can cause LiveIntervals / RA bookkeeping mismatches downstream.

## Source
`llvm/lib/CodeGen/InlineSpiller.cpp:1023-1037` (ImpReg captured in scan)
`llvm/lib/CodeGen/InlineSpiller.cpp:1161-1170` (strip loop)

```cpp
// Scan operands collected for folding:
for (const auto &OpPair : Ops) {
  ...
  MachineOperand &MO = MI->getOperand(Idx);
  ...
  if (MO.isImplicit()) {
    ImpReg = MO.getReg();   // OVERWRITTEN on each iteration
    continue;
  }
  ...
}

// After fold:
if (ImpReg)
  for (unsigned i = FoldMI->getNumOperands(); i; --i) {
    MachineOperand &MO = FoldMI->getOperand(i - 1);
    if (!MO.isReg() || !MO.isImplicit())
      break;
    if (MO.getReg() == ImpReg)
      FoldMI->removeOperand(i - 1);
  }
```

## Description
`ImpReg` is a single `Register` value, overwritten each time we encounter an implicit operand in `Ops`. The post-fold strip loop only removes implicit operands that equal that single `ImpReg`. If `MI` had multiple implicit operands attached to the original spilled vreg (which can happen with vector compose/decompose sequences, AMX tile loads, or tied/early-clobber super-register propagation), only one of them gets stripped from `FoldMI`.

Also, the strip loop `break`s on the first non-implicit operand seen, walking backward. If `FoldMI` ends with a non-implicit operand (which X86 fold tables produce for some patterns where the immediate is appended last), the strip aborts immediately and ImpReg's implicit operand is never removed.

The leftover implicit operands carry the original VREG number (`Reg` of the spilled register). After this returns, `spillAroundUses` does NOT touch `FoldMI`'s operands — the spilled vreg is supposed to be entirely gone from any users. But a stale implicit use of the spilled vreg remains. Later passes that walk MI operands (LiveDebugVariables, VirtRegRewriter) will see a virtual register operand that has no live interval after the spill is complete.

## Severity
Likely silent assertion / verifier failure in builds with expensive checks (`-verify-machineinstrs`), or worse, a stale vreg use that triggers `unreachable` in the rewriter. With `MO.getReg() == ImpReg` mismatching the actual ImpReg, the leftover implicit operands often reference the spilled vreg ID — which has been ERASED from `MachineRegisterInfo` by the spiller. Any later code that calls `MRI.getRegClass(Reg)` on that stale operand will segfault.

## Reproducer attempt
Targets that produce multiple implicit operands per use of a vreg are AMX, RISCV vector, AArch64 SVE, and some X86 compose patterns. Pure X86 -O2 with `<2 x i64>` may suffice if the spilled reg has tied early-clobber super-register implicit-defs:

```ll
define <2 x i64> @t(<2 x i64> %a, <2 x i64> %b, <2 x i64> %c, <2 x i64> %d,
                    <2 x i64> %e, <2 x i64> %f, <2 x i64> %g, <2 x i64> %h,
                    <2 x i64> %i, <2 x i64> %j, <2 x i64> %k, <2 x i64> %l,
                    <2 x i64> %m, <2 x i64> %n, <2 x i64> %o, <2 x i64> %p,
                    <2 x i64> %q) {
  %r1 = add <2 x i64> %a, %b
  %r2 = add <2 x i64> %r1, %c
  ...
  %use1 = mul <2 x i64> %q, %a
  %use2 = mul <2 x i64> %q, %b
  ret <2 x i64> %use1
}
```
forcing fold of a vreg with multiple implicit uses from extract-subreg patterns.

## Fix sketch
Change `ImpReg` to a `SmallSet<Register, 4>` (or `SmallVector`):
1. Collect ALL implicit operands' regs into the set during the scan.
2. In the strip loop, remove every implicit operand whose reg is in the set, and don't `break` on the first non-implicit — `continue` instead.

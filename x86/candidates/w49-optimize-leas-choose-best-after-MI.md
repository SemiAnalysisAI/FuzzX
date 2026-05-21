# X86OptimizeLEAs: `chooseBestLEA` may pick a LEA that the algorithm cannot legally lift across MI

## File
`llvm/lib/Target/X86/X86OptimizeLEAs.cpp`, lines 347-399 and 538-554.

## Code

```cpp
// chooseBestLEA: only checks LEA def-reg class compat and 1-byte disp
// preference; does NOT check whether the LEA's *address operands* are still
// defined and unmodified at the lift point above MI.

// removeRedundantAddrCalc:
//     If LEA occurs before current instruction, we can freely replace
//     the instruction. If LEA occurs after, we can lift LEA above the
//     instruction and this way to be able to replace it. Since LEA and the
//     instruction have similar memory operands (thus, the same def
//     instructions for these operands), we can always do that, without
//     worries of using registers before their defs.
if (Dist < 0) {
  DefMI->removeFromParent();
  MBB->insert(MachineBasicBlock::iterator(&MI), DefMI);
  InstrPos[DefMI] = InstrPos[&MI] - 1;
  ...
}
```

## Bug

The comment claims "we can always do that" based on the invariant that the LEA's address operands and MI's address operands share the same def instructions (because `MemOpKey::operator==` requires identical non-physical operand registers). But this argument fails for **physical registers** that appear in both:

- The `isIdenticalOp` helper used by `MemOpKey::operator==` *rejects* physical-register equality: `MO1.isIdenticalTo(MO2) && (!MO1.isReg() || !MO1.getReg().isPhysical())`. So two LEAs that differ in a physical address base register (e.g., one uses `%rip`, the other doesn't) won't share a bucket — fine.
- However, `MemOpKey` also keys on `AddrIndexReg` and `AddrSegmentReg`. If both operands are `X86::NoRegister`/0, that's the same. If both are some physical reg (e.g., `%fs` segment), the key match degenerates because `isIdenticalOp` returns false for physregs, *but the key only requires `isIdenticalOp` to return true*. So they end up in different buckets. Good, in this direction.

The real bug: the LEA being lifted *above* MI has its own *non-address* uses below MI but at least one address operand defined **between** MI and the LEA. When the LEA is lifted to immediately above MI, that intermediate def is now downstream of the LEA's new position — but the LEA reads the input value at its old position, not the new one. Concretely:

```
%mi_inst = ... ; uses some address with base %0
...
%0 = ...      ; redefines %0 (MI does not use this def directly)
%lea = LEA ... %0 ...   ; this LEA uses the redefined %0
```

If `chooseBestLEA` matches `%lea`'s memory key to `%mi_inst`'s key (both share the *same SSA name `%0`* at the IR level — both `MachineOperand`s are `isIdenticalTo` for vregs because their reg numbers are equal), the algorithm lifts `%lea` to immediately before `%mi_inst`. But `%0` had been re-defined between `%mi_inst` and `%lea`'s original position. After the lift, `%lea` reads the *old* `%0`, which changes the semantics of `%lea`.

Wait — at the MachineInstr level, the SSA invariant means there is exactly one def of `%0`. So if `%0` is redefined, those are *two distinct vregs* and `isIdenticalOp` would return false (different reg numbers). So in pure SSA pre-regalloc IR, the comment's claim holds.

The hole is **physical-register kills**. If the LEA's address base is a non-allocatable physreg like `%rip` (used for RIP-relative addressing) or a callee-saved physreg, both LEAs may reference it identically — but `isIdenticalOp` returns false for physregs, so they wouldn't share a bucket. So that case is also closed.

Where this can still be wrong: **MachineOperands carrying side state that participates in `isIdenticalTo` but not in `MemOpKey` hashing.** Consider `AddrSegmentReg = $noreg` vs. `AddrSegmentReg = $noreg` with different `RegState::Implicit` — `isIdenticalTo` requires flag equality. Two LEAs that both use `$noreg` for segment but with different implicit flags wouldn't share a bucket. OK.

After this analysis I believe the lift is in fact safe for vreg-only inputs. **However**, the comment understates a real precondition: this pass must run before register allocation. If `runOnMachineFunction` is ever invoked post-RA on a function with physreg address bases, the SSA invariant no longer protects the lift.

## Status

I am downgrading this candidate to a **documentation/assertion gap**: the pass should `assert(MRI->isSSA())` (or check `MachineFunctionProperties::IsSSA`) before performing the lift in `removeRedundantAddrCalc`, otherwise a future pass-pipeline reorder could silently violate the lift's safety argument.

## Confidence

Low. No miscompile reproducer; this is a maintainability/safety-argument concern.

# PeepholeOptimizer::foldImmediate visits all non-def regs incl. tied operands

File: `llvm/lib/CodeGen/PeepholeOptimizer.cpp:1462-1496`

## Reasoning

```cpp
bool PeepholeOptimizer::foldImmediate(
    MachineInstr &MI, SmallSet<Register, 4> &ImmDefRegs,
    DenseMap<Register, MachineInstr *> &ImmDefMIs, bool &Deleted) {
  Deleted = false;
  for (unsigned i = 0, e = MI.getDesc().getNumOperands(); i != e; ++i) {
    MachineOperand &MO = MI.getOperand(i);
    if (!MO.isReg() || MO.isDef())
      continue;
    ...
    if (TII->foldImmediate(MI, *II->second, Reg, MRI)) {
      ...
    }
  }
  return false;
}
```

`foldImmediate` iterates `MI.getDesc().getNumOperands()` (only *explicit*
operands) and skips defs. It does **not** check `MI.isRegTiedToDefOperand(i)`.
On x86, two-address instructions tie their use operand to the def operand
(e.g. ADD32rr operand 1 is tied to operand 0). If the immediate were folded
into the tied use operand, the resulting instruction `MOV32ri imm` would be a
new instruction the x86 target's `foldImmediate` is supposed to create — but
the target hook is expected to refuse such folds.

The risk is that the target hook (`X86InstrInfo::foldImmediate`) may not
always refuse: it forwards to `ConvertToImmediate`, and for some opcodes
(e.g. NDD variants `ADD32rr_ND`, `SUB32rr_ND`) the rr→ri conversion is
explicitly allowed via `convertALUrr2ALUri` even when the use is the *first*
source operand. For non-NDD ops the rr operand 1 is tied; for NDD it isn't.

The cross-class risk: if PeepholeOptimizer offers immediate-folding for the
tied operand of a non-NDD opcode and the target accepts (e.g., commutes
silently), the tied-def-and-use semantics are broken — the new instruction
defines a different register from its tied use, but the surrounding RA
machinery still believes the tie holds.

## Where it can bite

This is fundamentally a contract between PeepholeOptimizer (which trusts the
target hook) and `X86InstrInfo::foldImmediate`. The target hook does have a
guard for tied operands inside `convertALUrr2ALUri` (it bails when commute is
required for a tied operand). But the chain is fragile: any future opcode
added without the tied-operand check at the target side becomes a bug.

## Repro sketch

```mir
%0:gr32 = MOV32ri 1234
%1:gr32 = COPY %someother
%1:gr32 = ADD32rr %1, %0, implicit-def $eflags   ; %1 use is tied to %1 def
```

If `TII->foldImmediate` rewrites this to `ADD32ri %1, 1234, implicit-def $eflags`
without checking that operand 1 was tied, fine. But if the candidate is the
tied operand 1 (some opcodes do support `commute` to allow rr-imm folds on the
"other" source), and the rewrite drops the tie, the resulting MI is malformed.

## Expected wrong outcome

A register-allocator crash or wrong-register-class assert when the post-fold
MI has an explicit operand previously assumed tied. Worst case: silent
miscompile if the def register is materialized into one physreg and the use
into another but the target instr semantics still require they alias.

## Severity

Latent. Fileable because the loop body in PeepholeOptimizer::foldImmediate
itself never checks tie-ness or commutability before offering operand `i` to
the target hook, putting the entire correctness burden on each per-opcode
target check.

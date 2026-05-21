# MachineCopyPropagation::propagateDefs renames explicit def but leaves implicit-defs of Src

File: `llvm/lib/CodeGen/MachineCopyPropagation.cpp`,
function `MachineCopyPropagation::propagateDefs` (lines 1106-1172).

## Pattern

Backward propagation rewrites a def operand from `Src` to `Dst`:

```cpp
// llvm/lib/CodeGen/MachineCopyPropagation.cpp:1155-1156
    MODef.setReg(Dst);
    MODef.setIsRenamable(CopyOperands.Destination->isRenamable());
```

The pre-checks at line 1118 skip implicit MODef operands:

```cpp
    if (MODef.isTied() || MODef.isUndef() || MODef.isImplicit())
      continue;
```

The collision check at line 1145 (`hasOverlappingMultipleDef`) ensures MI
has no OTHER def that overlaps with the NEW register `Dst`:

```cpp
bool MachineCopyPropagation::hasOverlappingMultipleDef(
    const MachineInstr &MI, const MachineOperand &MODef, MCRegister Def) {
  for (const MachineOperand &MIDef : MI.all_defs()) {
    if ((&MIDef != &MODef) && MIDef.isReg() &&
        TRI->regsOverlap(Def, MIDef.getReg()))
      return true;
  }
  return false;
}
```

It only checks against `Def` (the NEW reg). It does **not** check that MI
lacks other defs of `Src` (the OLD reg), nor that the only def of Src's
register units is `MODef`.

## Bug scenario

Consider an instruction with both an explicit def of `Src` and an
implicit-def of a sub-register of `Src`:

```
MI:    $rsi = INSTR $rdi, implicit-def $sil   ; explicit def Src=$rsi
                                                ; implicit-def sub_8bit of $rsi
```

(This pattern can arise from X86 NDD/APX expansion, or from pseudo-expansion
that wants to model "this instruction also defines the low byte".)

After `propagateDefs` renames the explicit def to `Dst`:

```
MI:    $rdx = INSTR $rdi, implicit-def $sil   ; Dst=$rdx (was $rsi)
                                                ; implicit-def $sil still here!
```

The instruction now claims to define `$rdx` AND implicitly define `$sil`
(which is a sub-register of the OLD `$rsi`, not the NEW `$rdx`). This is a
semantic mismatch — the actual hardware behavior is that the instruction
writes to `$rdx`, not `$rsi`, but the MIR claims both.

`hasOverlappingMultipleDef(MI, MODef=$rsi-def, Dst=$rdx)` only checks
overlap with `$rdx`. The `implicit-def $sil` does not overlap with `$rdx`,
so the check passes. The rename proceeds.

## What about the SrcUsers loop?

The loop at line 1158-1165:

```cpp
    for (auto *SrcUser : Tracker.getSrcUsers(Src, *TRI)) {
      for (MachineOperand &MO : SrcUser->uses()) {
        if (!MO.isReg() || !MO.isUse() || MO.getReg() != Src)
          continue;
        MO.setReg(Dst);
        MO.setIsRenamable(CopyOperands.Destination->isRenamable());
      }
    }
```

This only iterates `SrcUser->uses()` — definitions are excluded. So
implicit-DEFS of Src on the rewriting MI are never renamed. The loop also
filters `MO.getReg() != Src` (exact match, no subreg consideration).

## Where could this trigger on x86?

Looking through X86 pseudo-instruction expansion (e.g. SUBREG_TO_REG,
`X86::INSERT_SUBREG`, NDD ops), a number of patterns emit explicit-def +
implicit-def-of-related-reg combinations. The exact path depends on whether
the rename target Dst is in a non-overlapping reg class with the implicit
def.

The most likely real-world trigger is an `implicit-def $eflags` (which is
already common) interacting with a same-class def rename — but $eflags is a
fixed reg with `isRenamable()==false` for most users, so `MODef.isRenamable`
gates wouldn't fire.

A more interesting case: AVX VEX/EVEX expansion may produce
`implicit-def $sub` for vector ops; rename across registers of those
classes can leave the implicit-def referring to a sub of the old reg.

## Source citation

```
llvm/lib/CodeGen/MachineCopyPropagation.cpp:1155-1156 (rename)
    MODef.setReg(Dst);
    MODef.setIsRenamable(CopyOperands.Destination->isRenamable());

llvm/lib/CodeGen/MachineCopyPropagation.cpp:1145-1146 (collision check)
    if (hasOverlappingMultipleDef(MI, MODef, Dst))
      continue;

llvm/lib/CodeGen/MachineCopyPropagation.cpp:778-786 (hasOverlappingMultipleDef
                                                     definition)
bool MachineCopyPropagation::hasOverlappingMultipleDef(
    const MachineInstr &MI, const MachineOperand &MODef, MCRegister Def) {
  for (const MachineOperand &MIDef : MI.all_defs()) {
    if ((&MIDef != &MODef) && MIDef.isReg() &&
        TRI->regsOverlap(Def, MIDef.getReg()))
      return true;
  }
  return false;
}
```

The fix is to also bail when MI has any OTHER def (explicit or implicit)
that overlaps with `MODef.getReg()` (the OLD register Src) — because
renaming only MODef leaves the other def referring to a dead register.

## Reproduction sketch

Direct MIR repro is hard because realistic implicit-def-of-related-reg
patterns require specific target lowering paths. The structural gap is
unambiguous from the source.

## Confidence

Medium. The structural gap (only checking collision against NEW reg, not
verifying OLD reg has no other defs) is clear. Practical trigger requires a
target/opcode that emits a non-tied implicit-def of a Src sub/super after RA,
which is uncommon but not unheard of.

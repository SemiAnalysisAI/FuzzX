# w354: X86 getConstValDefinedInReg recurses via SUBREG_TO_REG without verifying inner def def-class match

## Severity
Latent crash / wrong fold. Reachable when the MIR (e.g., after a custom pass, or after `BranchFolding` / `MachineCSE` glue) produces a `SUBREG_TO_REG ..., %src, sub_32bit` where `%src` is **not** GR32-class but still satisfies `MRI.getUniqueVRegDef` and reaches a `MOV32ri` / `MOV32ri64` / `MOV8ri` / `MOV32r0` switch case.

## Suspicious code
`llvm/lib/Target/X86/X86InstrInfo.cpp:4606-4643` — `X86InstrInfo::getConstValDefinedInReg`:

```cpp
if (MI.isSubregToReg()) {
  unsigned SubIdx = MI.getOperand(2).getImm();
  MovReg = MI.getOperand(1).getReg();
  if (SubIdx != X86::sub_32bit)
    return false;
  const MachineRegisterInfo &MRI = MI.getParent()->getParent()->getRegInfo();
  MovMI = MRI.getUniqueVRegDef(MovReg);                        // 4623
  if (!MovMI)
    return false;
}

if (MovMI->getOpcode() == X86::MOV32r0 &&
    MovMI->getOperand(0).getReg() == MovReg) {
  ImmVal = 0;
  return true;
}

if (MovMI->getOpcode() != X86::MOV32ri &&
    MovMI->getOpcode() != X86::MOV64ri &&
    MovMI->getOpcode() != X86::MOV32ri64 &&
    MovMI->getOpcode() != X86::MOV8ri)
  return false;
// Mov Src can be a global address.
if (!MovMI->getOperand(1).isImm() || MovMI->getOperand(0).getReg() != MovReg)
  return false;
ImmVal = MovMI->getOperand(1).getImm();
return true;
```

The function:
1. Checks `SubIdx == X86::sub_32bit` but **does not** check that the inner `MovReg` has a GR32-compatible register class. Targets where post-isel passes might rewrite `MovReg` to a different class (e.g., via `MachineRegisterInfo::constrainRegClass`) could end up resolving the SUBREG_TO_REG against an inner MOV*ri whose class is incompatible with the outer GR64.
2. Mixes `MOV8ri` into the same code path as `MOV32ri` / `MOV64ri`. `MOV8ri` writes only 8 bits with the **rest of the register unchanged** — it does NOT zero-extend like MOV32ri. If a SUBREG_TO_REG `%dst:gr64 = SUBREG_TO_REG %src:gr8, sub_32bit` (or via an intermediate GR32 vreg whose def is MOV8ri) reaches this code, the returned `ImmVal` describes only 8 bits of state, but the caller (`foldImmediateImpl`) treats it as the full 64-bit semantic value.

The MIR verifier checks `SUBREG_TO_REG`'s subreg vs the source class, so normal isel output won't produce a SUBREG_TO_REG sub_32bit over a GR8 source. But `getConstValDefinedInReg` does not re-verify this; if a future pass synthesizes such MIR (or if the SUBREG_TO_REG's source vreg is later widened by a class-constraining pass), the fold will silently mis-extend.

## Trigger conditions
- A SUBREG_TO_REG with `sub_32bit` whose source vreg's `MRI->getUniqueVRegDef` returns a `MOV8ri` instance.
- The corresponding GR64 result is used by a foldable use that PeepholeOptimizer's `foldImmediate` reaches.

Cannot be produced by mainline X86 isel: `MOV8ri` writes a GR8 vreg, and `SUBREG_TO_REG %gr8, sub_32bit` is a verifier error. The bug is latent against MIR-level fuzzing / future MIR-pass changes that don't preserve the implied class invariant.

## Probe IR
None — must be constructed at the MIR level. A `.mir` test could trigger the assert / wrong fold.

## Fix sketch
Guard the `MOV8ri` arm with a class check (require inner reg to be GR8-class **and** the outer subidx be `sub_8bit`):
```cpp
if (MovMI->getOpcode() == X86::MOV8ri) {
  if (MI.isSubregToReg() && SubIdx != X86::sub_8bit)
    return false;
}
```
and similarly require `sub_32bit` for `MOV32ri` / `MOV32ri64` / `MOV32r0`.

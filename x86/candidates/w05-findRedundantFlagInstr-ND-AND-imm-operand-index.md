# findRedundantFlagInstr assumes AND immediate at operand 2, missing ND variants & implicit-EFLAGS forms

File: llvm/lib/Target/X86/X86InstrInfo.cpp:1058-1140

## Description
`findRedundantFlagInstr` recognizes the pattern:

```
  %reg = AND32ri %x, K        ; sets EFLAGS, K fits in u16
  ...
  %src = COPY %reg.sub_16bit:gr32
  TEST16rr %src, %src         ; <-- candidate for removal
```

and the analogous TEST64rr/SUBREG_TO_REG form. The hard-coded check
(lines 1067-1070) is:

```cpp
if (!((VregDefInstr->getOpcode() == X86::AND32ri ||
       VregDefInstr->getOpcode() == X86::AND64ri32) &&
      isUInt<16>(VregDefInstr->getOperand(2).getImm())))
  return false;
```

Then later at line 1092 the broader `X86::isAND(VregDefInstr->getOpcode())`
guard is used and the entire AND def is treated as the EFLAGS source.

Two concerns:

1. **ND (NDD) variants ignored** (TEST16rr path only). `AND32ri_ND`
   and `AND64ri32_ND` (APX NDD non-destructive forms) have operand
   layout `%dst, %src, imm` where `imm` is also at index 2. They
   *also* set EFLAGS and would be valid candidates. But the explicit
   check at line 1067 lists only `AND32ri` / `AND64ri32`. Result: a
   missed optimization on APX. Mostly benign.

2. **Operand index 2 is not always the immediate.** Later at line 1092
   the path opens up to all `X86::isAND(...)` forms via the
   subreg_to_reg → TEST64rr leg (line 1073-1080 only verifies
   `getOperand(2).getImm() == X86::sub_32bit` for the SUBREG_TO_REG,
   which is structural — but then takes `VregDefInstr` and runs the
   broader isAND check without re-validating the operand layout).

   For the TEST64rr leg, the AND fed through SUBREG_TO_REG is reached
   via `VregDefInstr = MRI->getVRegDef(CmpValDefInstr.getOperand(1).getReg())`
   (line 1079). The hard-coded `isUInt<16>` AND-imm filter from the
   TEST16rr leg (line 1067-1070) is NOT applied to the TEST64rr leg.

   Now imagine `VregDefInstr` is `AND32rm` (memory-form AND) or
   `AND32rr_ND` (NDD register form), both of which match
   `X86::isAND(...)`. The code sets `NoSignFlag = true;
   ClearsOverflowFlag = true;` at lines 1136-1138 unconditionally,
   meaning the TEST64rr is removed and EFLAGS from this *non-immediate*
   AND is used directly.

   The result is correct *for the AND*'s SF/ZF semantics (AND clears
   OF, sets SF to high bit of result, ZF if zero). But the comment
   block at lines 1118-1135 explains why the immediate variants are
   safe: with an immediate ≤ 0xFFFF, the top 48 bits of the result are
   provably zero, so the SUBREG_TO_REG zero-extension matches what
   TEST64rr would test. With a non-immediate AND (AND32rr, AND32rm,
   AND32rr_ND), the upper bits *can* still be zero by construction of
   the 32-bit AND (x86 32-bit ops zero-extend to 64), so the
   SUBREG_TO_REG produces the same 64-bit value, AND ZF is preserved.

   The *real* concern is SF: TEST64rr reads SF from bit 63 of the
   value. After AND32, bit 63 is 0 (zero-extension). So
   `NoSignFlag = true` is fine — caller will refuse to fold this
   pattern for COND_S/COND_NS/COND_L/etc. Good.

   But: when `VregDefInstr` is from an AND of the form `AND32mr`
   (memory destination), the EFLAGS are still set on the *value
   stored*, but the value flowing to `CmpValDefInstr` via
   `getOperand(1)` is *not* the AND's result — `AND32mr` has no
   register def. `MRI->getVRegDef(...)` would not return AND32mr,
   though; AND32mr only defs memory, no virtual reg. So this path
   isn't hit.

The genuine subtle bug:

3. **EFLAGS clobber between VregDefInstr and SUBREG_TO_REG/COPY is
   checked, but the check uses `modifiesRegister(X86::EFLAGS, TRI)`
   which scans only the immediate range** `(VregDefInstr,
   CmpValDefInstr)`. The check is at lines 1108-1115:

```cpp
for (const MachineInstr &Instr :
     make_range(std::next(MachineBasicBlock::iterator(VregDefInstr)),
                MachineBasicBlock::iterator(CmpValDefInstr))) {
  if (Instr.modifiesRegister(X86::EFLAGS, TRI))
    return false;
}
```

This range does NOT include `CmpValDefInstr` itself. A
`SUBREG_TO_REG` is a no-op at machine level and does not modify
EFLAGS, OK. A `COPY` is also fine in general — but a COPY between
physical registers can lower to instructions that clobber EFLAGS
(`copyPhysReg` ends up generating MOV which is fine; but
sub-register COPYs to/from 16/8-bit physregs occasionally need
movzx/movsx + and). However, this all happens after register
allocation. At the SSA peephole stage where this fires, COPY is
just COPY.

The actually missing check is between **`CmpValDefInstr` and
`CmpInstr`**. The caller in optimizeCompareInstr is iterating
backward from CmpInstr; if some instruction between
CmpValDefInstr and CmpInstr modified EFLAGS, that instruction
would be visited *first* and either (a) return false at line
5458, or (b) be a MOV32r0 saved as `Movr0Inst`, or (c) be
`Inst.registerDefIsDead(X86::EFLAGS) + HasNF` queued for NF
rewrite. So this is OK for paths (a) and (c).

For path (b), `Movr0Inst` gets reordered later to live before
`Sub`. `Sub` here is `AndInstr` (the AND). The movr0 move-up logic
at lines 5631-5651 looks for an "Instr" that modifies EFLAGS but
doesn't read EFLAGS and inserts the MOV32r0 before it. If the AND
itself is in the range, it counts (AND modifies EFLAGS, doesn't
read). The MOV32r0 is then inserted *before* the AND. After
optimization the AND's EFLAGS def is now exposed to consumers
between AND and TEST (which the original code didn't allow to
read EFLAGS at all). But there are no consumers between AND and
TEST: the backward scan saw EFLAGS modify by MOV32r0, no other
EFLAGS use. OK this should be fine.

## Real concrete miscompile possibility — TEST16rr path with COPY
   from physreg

The TEST16rr leg requires `CmpValDefInstr.getOperand(1).getReg().isVirtual()`
(line 1059). Good.

## Net assessment
The most concrete issue is **(1)**: the ND variants of AND are not
considered, leading to a missed-optimization on APX targets but no
miscompile.

The latent concern is the asymmetry between the TEST16rr leg
(restricts to AND*ri with imm ≤ 0xFFFF) and the TEST64rr leg (uses
broader `X86::isAND` later without re-restricting). I cannot construct
a miscompile because zero-extension protects SF, and ZF/OF/CF semantics
are aligned, but this is a fragile pattern.

## Reproducer (MIR)
```
# llc -run-pass=peephole-opt -mtriple=x86_64-- -mattr=+ndd repro.mir
---
name: missed_opt
tracksRegLiveness: true
body: |
  bb.0:
    %0:gr32 = COPY $edi
    %1:gr32 = AND32ri_ND %0, 1, implicit-def $eflags    ; ND variant: not recognized
    %2:gr16 = COPY %1.sub_16bit
    TEST16rr %2, %2, implicit-def $eflags               ; should be removable but isn't
    JCC_1 %bb.2, 4, implicit $eflags
  bb.1:
    RET64
  bb.2:
    RET64
...
```
Wrong outcome: the TEST16rr is left in place even though it is
redundant with the ND AND's flags, costing one instruction.

## Verified
Confirmed with `llc -run-pass=peephole-opt -mattr=+ndd` on a hand
constructed MIR with `AND32ri_ND %0, 1, ... ; COPY .sub_16bit ;
TEST16rr`. The TEST16rr is *NOT* removed. Swapping `AND32ri_ND` for
plain `AND32ri` makes the TEST disappear as expected. So this is a
real missed-optimization on APX NDD targets.

## Suggested fix
Add `X86::AND32ri_ND` and `X86::AND64ri32_ND` to the operand-2 check
on lines 1067-1070. For the ND form the imm is at `getOperand(2)`
(operand layout: dst, src, imm). Note that for ND form the source we
should follow is `getOperand(1)` not the destination; current code
treats `CmpValDefInstr` (the COPY/SUBREG_TO_REG) — that's separately
correct because the COPY feeds from `%1:gr32` which IS the AND's
destination. The fix is one-line: add the two ND opcodes to the list.

# MachineLateInstrsCleanup merges instructions ignoring MachineMemOperands

File: `llvm/lib/CodeGen/MachineLateInstrsCleanup.cpp`,
function `Reg2MIMap::hasIdentical` (lines 45-49, calls `MI->isIdenticalTo`).

## Pattern

`MachineLateInstrsCleanup` collapses identical redundant defs across blocks
(and within blocks). The identity check is a thin wrapper around
`MachineInstr::isIdenticalTo`:

```cpp
// llvm/lib/CodeGen/MachineLateInstrsCleanup.cpp:45-49
  struct Reg2MIMap : public SmallDenseMap<Register, MachineInstr *> {
    bool hasIdentical(Register Reg, MachineInstr *ArgMI) {
      MachineInstr *MI = lookup(Reg);
      return MI && MI->isIdenticalTo(*ArgMI);
    }
  };
```

`MachineInstr::isIdenticalTo` (in `llvm/lib/CodeGen/MachineInstr.cpp:2052`)
compares opcode, number of operands, each operand's value (and optionally
kill/dead flags), pre/post symbols, debug-loc for debug instrs, CFI types
for calls. **It does NOT compare `MachineMemOperand`s.**

The neighbouring helper `hasIdenticalMMOs`
(`llvm/lib/CodeGen/MachineInstr.cpp:418`) exists precisely for this — but
`isIdenticalTo` never calls it, and `MachineLateInstrsCleanup` never invokes
it as a follow-up check.

## Eligibility of memory-touching instructions

`isCandidate` (lines 176-197) gates by `MI->isSafeToMove(SawStore)` with
`SawStore=true` as the input. `isSafeToMove` for a `mayLoad()` instruction
returns `!SawStore` UNLESS the load is `isDereferenceableInvariantLoad()`.
So the candidate set is restricted to:

1. Invariant/dereferenceable loads (e.g. constant-pool loads marked
   `MOInvariant` + `MODereferenceable`).
2. Non-memory instructions like LEA, MOV*ri (load address / immediate),
   which have no MMOs (harmless for this bug class).

The bug-relevant case is (1). On x86, `MOV{8,16,32,64}rm` and vector loads
that materialize a constant from `.rodata` typically carry MMOs with
`MachinePointerInfo::getConstantPool()` PseudoSourceValue, `MOLoad |
MOInvariant | MODereferenceable`. AAMD metadata (`!tbaa`, `!alias.scope`,
`!noalias`) on the original IR load propagates through to the MMO.

Concrete candidate examples on x86:
- Constant-pool loads: `MOV32rm <constpool>, ...` with
  `MOInvariant|MODereferenceable`.
- Vector constant loads: `MOVAPSrm <constpool>, ...`,
  `VMOVAPSZ128rm <constpool>, ...`.

## Where MMO divergence comes from

Two identical-looking loads in different BBs can carry different MMOs:
- `!nontemporal` on one but not the other (`MachineMemOperand::MONonTemporal`).
- Different alignment (`getAlign()`).
- Different `AAMDNodes` (TBAA / `!noalias` / `!alias.scope`).
- Different `MachinePointerInfo` (different PseudoSourceValue, e.g.
  FixedStack vs PseudoSourceValue::Stack).
- `MOInvariant` / `MODereferenceable` differing.

If predecessor blocks each define a Reg via such a load, and the entry-loop
at line 207-217 finds the same-MachineInstr-pointer in all preds, the def is
inherited. But the more dangerous case is intra-block: line 237's
`MBBDefs.hasIdentical(DefedReg, &MI)` followed by line 240's
`removeRedundantDef` — the second load is dropped wholesale, and its MMO
metadata vanishes. The surviving load is the FIRST one, which may have less
restrictive MMOs (no nontemporal, looser alignment, less precise AA info).

Result: a `!nontemporal` reload is silently turned into a plain
temporal-cacheable reload. The hardware hint is lost. AA metadata mismatch
can also enable downstream reorderings that the original source forbade.

## Why this can fire in practice

The pass runs late (`MachineLateInstrsCleanup`), after most movement
opportunities. The redundant-def pattern usually arises from frame-index
elimination producing identical LEA chains across blocks (the documented use
case), but ALSO from rematerialization paths in the register allocator that
re-emit a reload at multiple sites. If one of those reloads originated from
a `!nontemporal` IR load and another from a plain load, the MIs may differ
ONLY in MMOs.

## Source citation

```
llvm/lib/CodeGen/MachineLateInstrsCleanup.cpp:45-49
  struct Reg2MIMap : public SmallDenseMap<Register, MachineInstr *> {
    bool hasIdentical(Register Reg, MachineInstr *ArgMI) {
      MachineInstr *MI = lookup(Reg);
      return MI && MI->isIdenticalTo(*ArgMI);
    }
  };
```

The fix mirrors what `BranchFolder::mergeOperations` learned (see existing
catalog bug #141): also require `hasIdenticalMMOs` (or `cloneMergedMemRefs`
the survivor with the dropped MI's MMOs to widen rather than narrow).

## Reproduction sketch

Construct (via MIR) two identical `MOV32rm $rsp, 1, $noreg, 0, $noreg`
candidates, one with a `(load (s32) from %stack.0)` MMO carrying
`MONonTemporal`, one without:

```mir
# RUN: llc -mtriple=x86_64-linux-gnu -run-pass=machine-latecleanup %s
---
name: trigger
tracksRegLiveness: true
stack:
  - { id: 0, size: 4, alignment: 4 }
body: |
  bb.0:
    successors: %bb.1, %bb.2
    JCC_1 %bb.1, 5, implicit-def $eflags
  bb.1:
    renamable $eax = MOV32rm $rsp, 1, $noreg, 0, $noreg ::
        (load (s32) from %stack.0, !nontemporal !0)
    JMP_1 %bb.3
  bb.2:
    renamable $eax = MOV32rm $rsp, 1, $noreg, 0, $noreg ::
        (load (s32) from %stack.0)
    JMP_1 %bb.3
  bb.3:
    RET 0, $eax
!0 = !{i32 1}
...
```

The two reloads are identical except for the MMO `MONonTemporal` flag. After
`MachineLateInstrsCleanup`, depending on which block is processed first,
the survivor may drop the nontemporal hint.

Note: an end-to-end .ll-driven trigger is hard because earlier passes
(SimplifyCFG, GVN) tend to merge or eliminate the loads before MIR. The
MIR-level repro above is the cleanest way to exhibit the gap.

## Confidence

High on the structural gap (the omission of MMO comparison in `isIdenticalTo`
+ `hasIdentical` is mechanical). Medium on the practical trigger reaching
this pass — earlier IR-level passes often collapse the duplicates first.

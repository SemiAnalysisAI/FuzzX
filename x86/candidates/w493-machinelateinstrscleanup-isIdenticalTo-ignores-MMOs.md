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

## Reproduction (confirmed)

```mir
# RUN: llc -mtriple=x86_64-linux-gnu -run-pass=machine-latecleanup %s
--- |
  @const = private unnamed_addr constant i32 42
  define i32 @trigger(ptr %p) { ret i32 0 }
...
---
name: trigger
tracksRegLiveness: true
body: |
  bb.0:
    renamable $eax = MOV32rm $rsp, 1, $noreg, 0, $noreg ::
        (dereferenceable invariant load (s32) from @const)
    renamable $eax = MOV32rm $rsp, 1, $noreg, 0, $noreg ::
        (non-temporal dereferenceable invariant load (s32) from @const)
    RET 0, $eax
...
```

Output after `machine-latecleanup`:
```
bb.0:
    renamable $eax = MOV32rm $rsp, 1, $noreg, 0, $noreg ::
        (dereferenceable invariant load (s32) from @const)
    RET 0, $eax
```

The non-temporal hint is dropped: the FIRST load (plain) survives, the
SECOND (non-temporal) is removed. The result is a plain temporal load even
though the source IR / MIR requested nontemporal. The order is
implementation-detail of the iteration (MBBDefs picks the first identical
def encountered, then later identicals are removed).

Reverse order (`(non-temporal ...)` first, then plain) keeps the
nontemporal: also dropping the second is wrong but in a less visible way —
it dropped the LATER nontemporal request rather than the survivor's
nontemporal-ness.

## Confidence

High on the structural gap (the omission of MMO comparison in `isIdenticalTo`
+ `hasIdentical` is mechanical) and confirmed in repro: a non-temporal load
silently becomes a plain load. End-to-end .ll triggering through the
default `-O2` pipeline requires constant-pool-load duplication that the
upstream passes (CSE / GVN at IR level, MachineCSE at MIR level) tend to
collapse before reaching `machine-latecleanup`, but post-RA rematerialization
or per-block reload duplication can land such pairs.

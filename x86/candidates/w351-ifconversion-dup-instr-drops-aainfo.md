# w351: IfConversion `CountDuplicatedInstructions` uses isIdenticalTo which ignores MMOs/AAInfo

## Severity
Latent miscompile. Affects targets that run `IfConversion` (ARM/AArch64/Hexagon/PowerPC primarily). X86 normally does **not** run IfConversion (only `EarlyIfConversion`, and even that's gated on `-x86-early-ifcvt`), so the bug is observable downstream only — but the explicit hunt list calls this out so it's recorded here.

## Suspicious code
`llvm/lib/CodeGen/IfConversion.cpp:761` and `:804` — duplicate-instruction counting:

```cpp
// in CountDuplicatedInstructions, head pass (line 761):
if (!TIB->isIdenticalTo(*FIB))
  break;
...
// tail pass (line 804):
if (!RTIE->isIdenticalTo(*RFIE))
  break;
```

`MachineInstr::isIdenticalTo` in `llvm/lib/CodeGen/MachineInstr.cpp:673-744` compares opcodes, operand reg/imm/etc, pre/post symbols, and (for calls) CFI type — but **does not** compare:
- `MachineMemOperand` list / `AAInfo` / alias scopes
- `MIFlag`s (NoSWrap/NoUWrap/NoFPExcept/...)
- Debug locations (except for debug-instr opcodes)

After `CountDuplicatedInstructions` returns, `MergeBlocks` (line 1872-1873) splices MBB1's prefix into the head and erases MBB2's prefix:
```cpp
BBI.BB->splice(BBI.BB->end(), &MBB1, MBB1.begin(), DI1);
MBB2.erase(MBB2.begin(), DI2);
```

The surviving instruction keeps only **MBB1's** MMO list. If MBB1 had `(load (s32) from %p, !alias.scope <scope_A>)` and MBB2 had `(load (s32) from %p, !alias.scope <scope_B>)` — both deemed duplicates because the operand list is the same — the merged result drops `<scope_B>`. Downstream MachineScheduler / DAG combine / hoisting may now reorder the merged load with stores tagged `!noalias <scope_B>`, which was forbidden in the MBB2 path.

Similarly, `MIFlag::NoFPExcept` (`fadd nnan nofpx ... `) being asymmetric is silently lost: the surviving copy keeps MBB1's flags, the MBB2 instance's flags are discarded.

## Probe IR
On AArch64 / ARM where IfConversion runs. For X86 — not currently observable through normal `-O2` because the IfConversion pass is not in the X86 pipeline. Demonstrating on x86 would require `-x86-early-ifcvt` (which uses `SSAIfConv` not this code path) or a custom pipeline.

## Root cause summary
Duplicate detection conflates "operands match" with "MMOs and flags match". When the deletion side carries stricter `AAInfo` / `MIFlag` constraints, those are silently dropped during merge.

## Fix sketch
At lines 761 / 804, additionally require:
- `TIB->memoperands() == FIB->memoperands()` (or stricter intersection)
- `TIB->getFlags() == FIB->getFlags()`

Or: merge the surviving instruction's MMOs/flags with the deleted instance via the existing helper used by other code paths (e.g., `MachineInstr::cloneMergedMemRefs`) and intersect the `MIFlag` bitset to the safer (i.e., not asserting stricter than either parent) value.

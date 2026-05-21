# w46: AsmPrinter::emitCFIInstruction — "trailing CFI" skip uses MF.rbegin() instead of MBB->isEndSection()

File: llvm/lib/CodeGen/AsmPrinter/AsmPrinter.cpp

## Location

Lines 1416-1440:

```c++
void AsmPrinter::emitCFIInstruction(const MachineInstr &MI) {
  ...
  // If there is no "real" instruction following this CFI instruction, skip
  // emitting it; it would be beyond the end of the function's FDE range.
  auto *MBB = MI.getParent();
  auto I = std::next(MI.getIterator());
  while (I != MBB->end() && I->isTransient())
    ++I;
  if (I == MBB->instr_end() &&
      MBB->getReverseIterator() == MBB->getParent()->rbegin())
    return;
  ...
  emitCFIInstruction(CFI);
}
```

## Bug

The skip-emit condition is "I have no real instruction after me AND my
MBB is the last MBB in the function." The comment says "beyond the end
of the function's FDE range." But with `-fbasic-block-sections` (or
`-funique-section-names` with split sections, or HotColdSplit), one
function may have **multiple FDEs**, one per section. The FDE for each
section ends at the section's last MBB, not the function's last MBB.

A CFI instruction at the tail of the last MBB *of a non-final section*
(`MBB->isEndSection() == true`, but `MBB->getReverseIterator() !=
MF->rbegin()`) escapes the skip and will be emitted past the end of its
own FDE, producing assembler errors like:

  error: this directive must appear between .cfi_startproc and
         .cfi_endproc directives

or silently associating the CFI op with the next section's FDE.

Conversely, a CFI at the tail of the final MBB of the function is
correctly skipped only when no real follow-up exists — but only for the
final section. The check is also "off by one section" for cold split
parts emitted before the hot tail.

The structural fix is to use `MBB->isEndSection()` (or
`MBB->isEndSection() && std::next(MBB->getIterator()) == MF->end()`
depending on the desired semantics), not raw `rbegin()`.

## Reproducibility

Source-level finding. To reproduce in x86, need
`-mllvm -basic-block-sections=all` (or function sections + HotColdSplit
producing distinct CFI sections) on a function whose last MBB in a
non-final section has a trailing `CFI_INSTRUCTION` pseudo with no
following non-transient instructions. Frame lowering can emit
`cfi_remember_state` / `cfi_restore_state` near block boundaries which
may trigger this. Has not been bisected; filed for the fuzzer to attempt
basic-block-sections combinations.

## Why it matches the listed pattern

Bug pattern: "Wrong CFI cancel/restore emission across funclets." Both
basic-block-sections and funclets create multi-FDE functions; the same
`rbegin()` check is wrong in both cases. (Funclets are handled by
separate AsmPrinter paths on Win64 SEH, but DwarfCFI funclets — e.g.,
Itanium-style cleanup — share this code path.)

## Fix sketch

Replace

```c++
MBB->getReverseIterator() == MBB->getParent()->rbegin()
```

with

```c++
MBB->isEndSection()
```

(and possibly additionally require this to be the function's last
section if the intent is strictly "no FDE for this CFI to live in").

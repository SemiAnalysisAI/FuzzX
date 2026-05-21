# w340: MachineCSE silently drops MI's MachineMemOperand on CSE-erase

## Summary
When MachineCSE finds a redundant load `MI` whose operands are `isIdenticalTo` an
earlier load `CSMI`, it replaces uses of `MI`'s defs with `CSMI`'s defs and then
calls `MI.eraseFromParent()` without ever merging `MI`'s MMO into `CSMI`. Because
`MachineInstr::isIdenticalTo` and `MachineInstrExpressionTrait::getHashValue`
**do not look at MMOs at all**, two loads from the same address whose MMOs differ
in `!range`, `!invariant.load`, `!nontemporal`, `!alias.scope`, `!noalias`, or
the `MOInvariant`/`MODereferenceable`/`MONonTemporal` flags will be CSE'd, and
only `CSMI`'s MMO survives.

The expected behavior (matched by SDAG's `DAG.UpdateNodeOperands` /
`MergeMemOperands` paths) is to **intersect** MMO flags and **union** AAMDNodes
across the two loads, since after CSE the single remaining load must be a
correct description of both originals. Today MachineCSE never updates the kept
MMO at all, so the surviving MMO can over-promise (e.g. `!range !1` even though
MI's `!range !2` was a different range that some downstream consumer relied on)
or under-promise (drop `!invariant.load`/`nontemporal` that MI had but CSMI
didn't).

## Root cause (source citations)

`llvm/lib/CodeGen/MachineCSE.cpp` `ProcessBlockCSE`, the CSE-elimination path:

- Line 670-681: replaces vreg defs, then
- Line 725: `MI.eraseFromParent();`

Nowhere in the loop is `CSMI->cloneMergedMemRefs()` /
`CSMI->setMemRefs(union)` called. The MMO of the eliminated MI is dropped.

The reason `MI` gets matched to `CSMI` despite differing MMOs:

- `llvm/lib/CodeGen/MachineInstr.cpp:673` `MachineInstr::isIdenticalTo` walks
  only `MachineOperand`s and pre/post-instr symbols. It does not iterate
  `memoperands()`.
- `llvm/lib/CodeGen/MachineInstr.cpp:2333` `MachineInstrExpressionTrait::getHashValue`
  hashes opcode + non-vreg-def `MachineOperand`s only. MMOs are not in the hash.

Compare this with `MachineMemOperand`'s own `operator==`
(`llvm/include/llvm/CodeGen/MachineMemOperand.h:349-360`) which DOES include
flags, `AAInfo`, ranges, alignment, addrspace -- so the framework is aware that
two MMOs can be unequal, MachineCSE just ignores it.

## Reproducer (MIR, runs machine-cse only)

```
--- |
  target triple = "x86_64-unknown-linux-gnu"
  define i32 @f(ptr %p) { ret i32 0 }
  !1 = !{i32 0, i32 100}
  !2 = !{i32 200, i32 300}
...
---
name: f
tracksRegLiveness: true
registers:
  - { id: 0, class: gr64 }
  - { id: 1, class: gr32 }
  - { id: 2, class: gr32 }
  - { id: 3, class: gr32 }
liveins:
  - { reg: '$rdi', virtual-reg: '%0' }
body: |
  bb.0:
    liveins: $rdi
    %0:gr64 = COPY $rdi
    %1:gr32 = MOV32rm %0, 1, $noreg, 0, $noreg :: (dereferenceable invariant load (s32) from %ir.p, !range !1)
    %2:gr32 = MOV32rm %0, 1, $noreg, 0, $noreg :: (dereferenceable invariant load (s32) from %ir.p, !range !2)
    %3:gr32 = ADD32rr %1, %2, implicit-def dead $eflags
    $eax = COPY %3
    RET 0, $eax
...
```

Command:
```
llc -run-pass=machine-cse test_cse4.mir -o -
```

After CSE:
```
%1:gr32 = MOV32rm %0, 1, $noreg, 0, $noreg :: (dereferenceable invariant load (s32) from %ir.p, !range !1)
%3:gr32 = ADD32rr %1, %1, implicit-def dead $eflags
```

The MMO of the second load (which carried `!range !2 = [200,300)`) is gone --
only `!range !1 = [0,100)` survives. A later pass that consumes the kept load's
range now believes the value is in `[0,100)`, but the original program had a use
that was annotated `[200,300)`; if any consumer of the original `%2` was
specialized for that range (e.g. via `KnownBits`-based folding in
`X86DAGToDAGISel` / `MachineCombiner` re-runs, or via target hooks that look at
MMO ranges), it is now operating on incorrect range data.

## Variants this can produce
- Drop of `!invariant.load`: a later pass that would have hoisted/CSE'd a
  third copy no longer can. Missed opt only.
- Drop of `MONonTemporal`: emitted code uses temporal `MOV` instead of
  `MOVNT*`, losing the perf hint. Perf only.
- Drop of `!alias.scope` / `!noalias` (in the `AAInfo` of MI's MMO): downstream
  `MachineInstr::mayAlias` queries become more conservative -> missed opt; in
  the reverse direction (CSMI's MMO stays, MI's tighter aliasing is lost),
  later loads/stores cannot be reordered past the kept load even when the
  program proves they don't alias.
- Drop of stricter `!range` (the reproducer above): MIR consumers see the
  wrong range and may fold to a wrong constant. This is a potential
  correctness issue once any post-CSE pass starts reading `MMO->getRanges()`.

## Fix sketch
In the `if (DoCSE)` block in `ProcessBlockCSE`
(`MachineCSE.cpp` ~line 670), before `MI.eraseFromParent()` call
`CSMI->cloneMergedMemRefs(*MF, {CSMI, &MI})` (or implement an explicit
MMO-intersect of flags, AAInfo union, range union, align min).

## Cite
- `llvm/lib/CodeGen/MachineCSE.cpp:670-725` (DoCSE: replace defs then erase MI,
  no MMO merge)
- `llvm/lib/CodeGen/MachineInstr.cpp:673-740` (`isIdenticalTo` ignores MMOs)
- `llvm/lib/CodeGen/MachineInstr.cpp:2332-2345` (`getHashValue` ignores MMOs)
- `llvm/include/llvm/CodeGen/MachineMemOperand.h:349-360` (MMO `operator==`
  shows the fields that ARE meant to differ)

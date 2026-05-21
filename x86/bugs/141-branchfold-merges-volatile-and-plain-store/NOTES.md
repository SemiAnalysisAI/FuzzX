# BranchFolding tail-merge merges a volatile and a non-volatile store into one MOV32mi

File: `llvm/lib/CodeGen/BranchFolding.cpp:818-822` (assertion + `cloneMergedMemRefs`)
Also: `llvm/lib/CodeGen/MachineInstr.cpp:673` (`MachineInstr::isIdenticalTo` — does
**not** compare MachineMemOperand flags such as `isVolatile`, `isAtomic`, or
ordering, only `getNumOperands`/opcode/operands).

## Bug

In `BranchFolder::mergeOperations`:
```cpp
assert(MBBICommon->isIdenticalTo(*MBBI) && "Expected matching MIIs!");

// Merge MMOs from memory operations in the common block.
if (MBBICommon->mayLoadOrStore())
  MBBICommon->cloneMergedMemRefs(*MBB->getParent(), {&*MBBICommon, &*MBBI});
```

`isIdenticalTo` returns true for two stores whose register/operand list is
identical but whose MMOs differ in volatility/ordering. The two stores
are then merged and `cloneMergedMemRefs` concatenates the MMOs. The resulting
instruction has the side-effect set of every MMO present (the strictest), but
the surviving MachineInstr also forgets which path required volatile
semantics — and the merge happens silently even when one of the original
two stores was non-volatile.

This becomes a real miscompile when later passes inspect MMOs individually
(e.g., `MachineCSE::isCSECandidate` only looks at `mayStore`, but mayAlias
analyses and the X86 backend's reorder/fold-into-load logic walk MMO-by-MMO
and may choose the "wrong" MMO).

## IR repro

```ll
target triple = "x86_64-linux-gnu"

define void @f(i1 %c, ptr %p) {
entry:
  br i1 %c, label %then, label %else
then:
  store volatile i32 42, ptr %p, align 4
  br label %end
else:
  store i32 42, ptr %p, align 4
  br label %end
end:
  ret void
}
```

Run:
```
llc -O2 -mtriple=x86_64-linux-gnu -print-after=branch-folder vol.ll -o /dev/null
```

## Observed merge

Before branch-folder:
```
bb.1.then:
  MOV32mi killed renamable $rsi, 1, $noreg, 0, $noreg, 42 :: (volatile store (s32) into %ir.p)
bb.2.else:
  MOV32mi killed renamable $rsi, 1, $noreg, 0, $noreg, 42 :: (store (s32) into %ir.p)
```

After branch-folder (single instruction in `bb.0`):
```
bb.0.entry:
  TEST8ri renamable $dil, 1, implicit-def $eflags, implicit killed $edi
  MOV32mi killed renamable $rsi, 1, $noreg, 0, $noreg, 42 ::
      (store (s32) into %ir.p), (volatile store (s32) into %ir.p)
  RET 0
```

The merged instruction now carries **both** a `(store)` MMO and a
`(volatile store)` MMO for the same address. `hasOrderedMemoryRef()` returns
true (correct, conservative), but the instruction is no longer distinguishable
from "always-volatile" by mayAlias queries that pick a specific MMO, and any
later analysis that loops over MMOs sees a non-volatile MMO covering the same
address — silently licensing a transform forbidden on the `then` path.

## Root cause / fix sketch

`MachineInstr::isIdenticalTo` predates the MMO `Flags` field and ignores it.
`BranchFolder::ComputeCommonTailLength` (the gate at line 370) should reject
merge candidates whose MMO flag sets disagree on the union of
`MOVolatile`/`MOAtomic`/ordering bits, OR `mergeOperations` should refuse to
combine MMOs whose volatility differs (and instead conservatively drop them
entirely, leaving `memoperands_empty()` to force a fully-conservative reading).

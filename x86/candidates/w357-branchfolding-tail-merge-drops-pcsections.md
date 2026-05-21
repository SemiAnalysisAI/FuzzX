# w357: BranchFolding tail-merge silently drops !pcsections (observable)

## Pass / Target
- BranchFolding tail-merge (`-O2` default), x86_64
- llc: `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc` (LLVM 23.0.0git)

## Root cause

`MachineInstr::isIdenticalTo`
(`llvm/lib/CodeGen/MachineInstr.cpp:673-765`) compares opcode, operands, debug
info (for debug instrs), pre/post-instr symbols, CFI-type and deactivation
symbol (for calls). It does **not** compare:
- `getPCSections()`
- `getMMRAMetadata()`
- `getHeapAllocMarker()`
- `getCFIType()` (for non-call instructions)
- memoperands

`BranchFolder::mergeCommonTails`
(`llvm/lib/CodeGen/BranchFolding.cpp:838-873`) merges identical instructions
across blocks. For memory ops it calls `cloneMergedMemRefs`
(`llvm/lib/CodeGen/BranchFolding.cpp:821-822`) but nothing equivalent is done
for the other MI-level extra-info fields above. The kept (`commonTailIndex`)
block's instruction survives unmodified; the donor instructions' `pcsections`,
`MMRAs`, etc. are simply lost when the donor block is rewritten with
`replaceTailWithBranchTo` (`llvm/lib/CodeGen/BranchFolding.cpp:1033`).

## Reproducer (.ll)

```llvm
target triple = "x86_64-unknown-linux-gnu"

define void @f(i1 %c, ptr %p) {
entry:
  br i1 %c, label %T, label %F
T:
  store i32 1, ptr %p, align 4, !pcsections !1
  br label %done
F:
  store i32 1, ptr %p, align 4
  br label %done
done:
  ret void
}
!0 = !{i64 0}
!1 = !{!"a-section", !0}
```

Command:
```
llc -O2 -mtriple=x86_64-unknown-linux-gnu repro.ll -o -
```

### Output (relevant portion)
```
f:                                      # @f
        testb   $1, %dil
        movl    $1, (%rsi)
        retq
```

There is no `.Lpcsection0:` label and no `.section "a-section",...` directive
at all.

### Same source at `-O0` (no BranchFolding):
```
.LBB0_1:                                # %T
        movq    -8(%rsp), %rax
.Lpcsection0:                                   ; <-- emitted
        movl    $1, (%rax)
        ...
        .section        "a-section","awo",@progbits,.text
.Lpcsection_base0:
        .long   .Lpcsection0-.Lpcsection_base0
        .quad   0                               # 0x0
```

### MIR diff
Before BranchFolding:
```
bb.1.T:
  MOV32mi $rsi, 1, $noreg, 0, $noreg, 1, pcsections !0 :: (store (s32) into %ir.p)
bb.2.F:
  MOV32mi $rsi, 1, $noreg, 0, $noreg, 1                  :: (store (s32) into %ir.p)
```
After:
```
bb.0.entry:
  MOV32mi $rsi, 1, $noreg, 0, $noreg, 1                  :: (store (s32) into %ir.p)
```

The `pcsections !0` annotation on bb.1's store has disappeared from the merged
instruction.

## Why this is a bug

`!pcsections` is used by kernel sanitizers (KCSAN, KCFI, etc.) and other
out-of-band runtime instrumentations to record the PC of specific stores into a
section table for later analysis. Dropping the annotation means the store is
silently removed from the instrumentation table — i.e., the runtime
instrumentation believes the store is not present. This is an observable
correctness loss in the emitted object file (a section is missing, a
`.Lpcsection*` label is missing).

The fact that the kept-block's annotation is the only one that survives means
the bug is asymmetric: if T (annotated) becomes the common-tail kept block, the
annotation is preserved; if F (unannotated) wins, it is silently dropped. The
choice is dictated by the heuristic in `TryTailMergeBlocks`
(`llvm/lib/CodeGen/BranchFolding.cpp:980-998`) which is layout/predecessor
driven and not stable across small CFG changes.

The same root-cause defect applies to `!mmra` (AMDGPU memory model regions),
`getHeapAllocMarker`, and the non-call `getCFIType`.

## Suggested fix

Either:
1. Extend `MachineInstr::isIdenticalTo` to compare PCSections / MMRA / HeapAllocMarker / non-call CFIType. This prevents the merge entirely when annotations differ.
2. Or, in `mergeOperations` / `mergeCommonTails`, when annotations differ across the merged instructions, choose a conservative merge (drop the annotation entirely or keep the union, per annotation kind).

Option 1 mirrors the existing treatment of pre/post-instr symbols and is the
minimal fix.

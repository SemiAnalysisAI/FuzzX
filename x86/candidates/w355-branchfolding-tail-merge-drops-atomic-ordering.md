# w355: BranchFolding tail-merge drops MMO atomic ordering

## Pass / Target
- BranchFolding tail-merge (`-O2` default), x86_64
- llc: `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc` (LLVM 23.0.0git)

## Root cause

`MachineMemOperand::operator==`
(`llvm/include/llvm/CodeGen/MachineMemOperand.h:349-360`) compares
value/size/offset/flags/AAInfo/ranges/align/addrspace but does **not** compare
`getSuccessOrdering()`, `getFailureOrdering()`, or `getSyncScopeID()`.

`BranchFolder::mergeOperations`
(`llvm/lib/CodeGen/BranchFolding.cpp:789-836`) calls
`MBBICommon->cloneMergedMemRefs(MF, {&*MBBICommon, &*MBBI})` (line 822) on every
pair of memory ops being merged from the donor block into the kept "common
tail" block.

`MachineInstr::cloneMergedMemRefs`
(`llvm/lib/CodeGen/MachineInstr.cpp:429-478`) uses `hasIdenticalMMOs`
(`llvm/lib/CodeGen/MachineInstr.cpp:418-427`), which delegates to MMO
`operator==`. Because ordering is not part of equality, two MMOs that differ
*only* in atomic ordering are considered identical and the donor's ordering is
silently discarded — the kept (`MIs[0]`) MMO survives unchanged.

## Reproducer (.ll)

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @f(i1 %c, ptr %p) {
entry:
  br i1 %c, label %T, label %F
T:
  %a = load atomic i32, ptr %p monotonic, align 4
  br label %done
F:
  %b = load i32, ptr %p, align 4
  br label %done
done:
  %v = phi i32 [%a, %T], [%b, %F]
  ret i32 %v
}
```

Command:
```
llc -O2 -mtriple=x86_64-unknown-linux-gnu repro.ll -o - \
    -print-before=branch-folder -print-after=branch-folder
```

### Before BranchFolding
```
bb.1.T: liveins: $rsi
  $eax = MOV32rm killed $rsi, 1, $noreg, 0, $noreg :: (load monotonic (s32) from %ir.p)
  JMP_1 %bb.3
bb.2.F: liveins: $rsi
  $eax = MOV32rm killed $rsi, 1, $noreg, 0, $noreg :: (load (s32) from %ir.p)
```

### After BranchFolding (merged into bb.0)
```
bb.0.entry: liveins: $edi, $rsi
  TEST8ri $dil, 1, implicit-def $eflags, implicit killed $edi
  $eax = MOV32rm killed $rsi, 1, $noreg, 0, $noreg :: (load (s32) from %ir.p)
  RET 0, $eax
```

The `monotonic` ordering present on bb.1's MMO is gone — the merged
instruction is annotated as a plain non-atomic load.

## Why this is a bug

The MMO is the IR-level record of memory effects consumed by later passes
(scheduler, post-RA optimizations, AA queries that look at MMO atomicity such
as `MachineMemOperand::isUnordered()`). After this merge a downstream pass that
calls `isUnordered()` or `isAtomic()` will be told this load has no ordering
requirement, even though on the T-path the IR semantically requires a monotonic
load. This is a metadata-correctness violation; if a subsequent pass uses
`isUnordered()` to permit a transform that would otherwise be illegal for an
atomic op (e.g., splitting/widening, store-elimination across the load, etc.)
it will produce wrong code.

The same equality bug also drops `syncscope` (verified separately: a system
`monotonic` store on T merged with a `syncscope("singlethread") monotonic`
store on F yields a `syncscope("singlethread") monotonic` store — a system
atomic narrowed to single-thread scope, which weakens cross-thread visibility
guarantees).

## Suggested fix

`MachineMemOperand::operator==` should compare `getSuccessOrdering()`,
`getFailureOrdering()` and `getSyncScopeID()` (and arguably `getMMRAMetadata()`
which is also missing). Alternatively, `cloneMergedMemRefs` should refuse to
deduplicate any pair that differ in atomic flags, falling back to keeping both
MMOs (or to `dropMemRefs` as the conservative bottom).

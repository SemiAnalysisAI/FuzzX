# w356: BranchFolding tail-merge narrows MMO syncscope to weaker scope

## Pass / Target
- BranchFolding tail-merge (`-O2` default), x86_64
- llc: `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc` (LLVM 23.0.0git)

## Root cause

Same root cause as w355: `MachineMemOperand::operator==`
(`llvm/include/llvm/CodeGen/MachineMemOperand.h:349-360`) omits
`getSyncScopeID()` from the equality check. When `BranchFolder::mergeOperations`
(`llvm/lib/CodeGen/BranchFolding.cpp:821-822`) calls `cloneMergedMemRefs`,
`hasIdenticalMMOs` (`llvm/lib/CodeGen/MachineInstr.cpp:418-427`) treats two
otherwise-matching atomic MMOs as identical even when they have *different
syncscope IDs*. The kept block's MMO survives; the other is silently dropped.

Unlike ordering (where dropping monotonic→unordered just weakens metadata),
narrowing a `system`-scope atomic to `syncscope("singlethread")` weakens the
cross-thread visibility guarantee in MMO metadata. Downstream passes that
respect syncscope (alias analysis, scheduler treating cross-thread atomics
specially, machine-LICM, …) will now treat the operation as if no other thread
can observe it.

## Reproducer (.ll)

```llvm
target triple = "x86_64-unknown-linux-gnu"

define void @f(i1 %c, ptr %p) {
entry:
  br i1 %c, label %T, label %F
T:
  store atomic i32 1, ptr %p monotonic, align 4    ; system scope
  br label %done
F:
  store atomic i32 1, ptr %p syncscope("singlethread") monotonic, align 4
  br label %done
done:
  ret void
}
```

Command:
```
llc -O2 -mtriple=x86_64-unknown-linux-gnu repro.ll -o - \
    -print-before=branch-folder -print-after=branch-folder
```

### Before BranchFolding
```
bb.1.T:
  MOV32mi $rsi, 1, $noreg, 0, $noreg, 1 :: (store monotonic (s32) into %ir.p)
bb.2.F:
  MOV32mi $rsi, 1, $noreg, 0, $noreg, 1 :: (store syncscope("singlethread") monotonic (s32) into %ir.p)
```

### After BranchFolding
```
bb.0.entry:
  TEST8ri $dil, 1, implicit-def $eflags, implicit killed $edi
  MOV32mi $rsi, 1, $noreg, 0, $noreg, 1 :: (store syncscope("singlethread") monotonic (s32) into %ir.p)
  RET 0
```

The T-path semantically required a system-scope atomic store. After tail-merge
the unified store is annotated `syncscope("singlethread")`, weakening the
cross-thread visibility metadata for the bb.1 case.

## Why this is a bug

LLVM IR semantics: a system-scope monotonic store on the T path must be
observable in monotonic order by other threads. If a later pass uses the MMO's
`getSyncScopeID()` (e.g., AA/scheduler treating singlethread atomics as
thread-local and reorderable past other thread-local accesses), it will make an
unsafe choice.

The fact that the `commonTailIndex` choice (which block becomes the kept block)
is essentially a heuristic — driven by predecessor count / position in the
worklist — means the bug can flip on either way depending on CFG layout.

## Suggested fix

Include `getSuccessOrdering()`, `getFailureOrdering()`, and `getSyncScopeID()`
in `MachineMemOperand::operator==` so `cloneMergedMemRefs` keeps both MMOs (the
existing union-on-difference path) when they disagree on atomicity.

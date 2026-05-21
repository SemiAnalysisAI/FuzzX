# w84 -- SimplifyCFG sink-common-insts merges two `volatile` `seq_cst` atomicrmw into one

## Location

`llvm/lib/Transforms/Utils/SimplifyCFG.cpp` -- `sinkCommonCodeFromPredecessors`
(line 2388) and the underlying identity comparator `canSinkInstructions`
(line 2190) / `hasSameSpecialState` in `llvm/lib/IR/Instruction.cpp`.

For `AtomicRMWInst`, `hasSameSpecialState` requires only that the two RMWs
agree on the RMW operation, `isElementwise`, `isVolatile`, alignment,
`getOrdering` and `getSyncScopeID`. There is no check that prevents merging
two `volatile`/`seq_cst` atomicrmw instances even though each is
independently observable through `volatile` and through `seq_cst`
participation in the global order.

So two `atomicrmw volatile add ... seq_cst` in mutually-exclusive
predecessors of a join block whose only difference is the value operand
collapse to a single `atomicrmw volatile add ... seq_cst` with a `select`
of the two value operands.

This is wrong for the same two reasons as the cmpxchg variant: it reduces
the count of volatile accesses (language-observable), and it reduces the
count of `seq_cst` operations participating in the global S total order
(coherence-observable in concurrent programs).

## Repro

`/tmp/w84/sink_vol_atomicrmw_same.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @sink_vol_atomicrmw(i1 %c, ptr %p, i32 %v) {
entry:
  br i1 %c, label %then, label %else
then:
  %a = atomicrmw volatile add ptr %p, i32 %v seq_cst, align 4
  br label %end
else:
  %b = atomicrmw volatile add ptr %p, i32 %v seq_cst, align 4
  br label %end
end:
  %r = phi i32 [ %a, %then ], [ %b, %else ]
  ret i32 %r
}
```

## Invocation

```
opt -passes='simplifycfg' -S sink_vol_atomicrmw_same.ll
```

(The default simplifycfg already sinks the common last instruction; no
`<sink-common-insts>` option is required. With different value operands,
`<sink-common-insts>` is required and the merge introduces a `select` on
the value operand.)

## Output

```
define i32 @sink_vol_atomicrmw(i1 %c, ptr %p, i32 %v) {
entry:
  %a = atomicrmw volatile add ptr %p, i32 %v seq_cst, align 4
  ret i32 %a
}
```

Two distinct `atomicrmw volatile add ... seq_cst` instructions in the
source program -- corresponding, e.g., to two distinct C `__atomic_fetch_add`
calls on a `volatile _Atomic int` -- have been collapsed to a single one.

## Codegen confirmation

`llc -O0` on the pre-simplifycfg IR emits two `lock xaddl` instructions
(one per branch). After `opt -passes='simplifycfg<sink-common-insts>'`, the
post-simplifycfg IR produces a single `lock xaddl`, observably reducing
the number of locked read-modify-write transactions visible to other CPUs.

## Family

Same defect family as #119 (mergeConditionalStores drops atomic), #120/#121
(sink/hoist common volatile load/store), #122 (hoist seq_cst load), but
applied to `atomicrmw` -- a distinct LLVM instruction kind not covered
by the existing reports. Together with the cmpxchg companion candidate,
this completes the coverage gap: `hasSameSpecialState` allows hoist/sink
of every flavour of LLVM atomic memory instruction (`load`, `store`,
`cmpxchg`, `atomicrmw`) without considering the observability of
`volatile` or stronger-than-relaxed orderings.

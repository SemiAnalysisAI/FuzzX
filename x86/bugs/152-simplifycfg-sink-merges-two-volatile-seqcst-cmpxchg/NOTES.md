# w84 -- SimplifyCFG sink-common-insts merges two `volatile` `seq_cst` cmpxchg into one

## Location

`llvm/lib/Transforms/Utils/SimplifyCFG.cpp` -- `sinkCommonCodeFromPredecessors`
(line 2388) and the underlying identity comparator `canSinkInstructions`
(line 2190) / `hasSameSpecialState` (in `llvm/lib/IR/Instruction.cpp`).

For `AtomicCmpXchgInst`, `hasSameSpecialState` requires only that the two
cmpxchg agree on `isVolatile`, alignment, `isWeak`, success/failure ordering
and sync-scope. There is no check that prohibits merging two cmpxchg that
are each independently observable through `volatile` and through `seq_cst`
participation in the total order of `seq_cst` operations.

As a result, two `cmpxchg volatile ... seq_cst seq_cst` in mutually-exclusive
predecessors collapse to a single `cmpxchg volatile ... seq_cst seq_cst` in
the join block (pointer / cmp / new operands become phi/select inputs).

This is wrong for **both** reasons:
1. `volatile` -- the count and ordering of volatile accesses is language-
   observable and may not be reduced (same defect family as #w57-volatile
   stores / loads).
2. `seq_cst` -- each cmpxchg-success participates in the global S total
   order. Reducing the number of `seq_cst` ops can change the coherence-
   observable outcomes of concurrent programs.

## Repro

`/tmp/w84/sink_vol_cmpxchg.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"

define { i32, i1 } @sink_vol_cmpxchg(i1 %c, ptr %p, i32 %v, i32 %nv) {
entry:
  br i1 %c, label %then, label %else
then:
  %a = cmpxchg volatile ptr %p, i32 %v, i32 %nv seq_cst seq_cst, align 4
  br label %end
else:
  %b = cmpxchg volatile ptr %p, i32 %v, i32 %nv seq_cst seq_cst, align 4
  br label %end
end:
  %r = phi { i32, i1 } [ %a, %then ], [ %b, %else ]
  ret { i32, i1 } %r
}
```

## Invocation

```
opt -passes='simplifycfg' -S sink_vol_cmpxchg.ll
```

(The default simplifycfg already sinks the common last instruction; no
`<sink-common-insts>` option is required.)

## Output

```
define { i32, i1 } @sink_vol_cmpxchg(i1 %c, ptr %p, i32 %v, i32 %nv) {
entry:
  %a = cmpxchg volatile ptr %p, i32 %v, i32 %nv seq_cst seq_cst, align 4
  ret { i32, i1 } %a
}
```

The two original `cmpxchg volatile ... seq_cst seq_cst` collapsed into a
single one. The source program contains two distinct volatile / seq_cst
operations; LLVM produces IR with only one.

## Codegen confirmation

`llc -O0` on the pre-simplifycfg IR emits two `lock cmpxchgl` instructions
(one per branch). After `opt -passes='simplifycfg'`, the post-simplifycfg
IR produces a single `lock cmpxchgl`, observably reducing the number of
locked compare-exchange transactions visible to other CPUs.

## Family

Same defect family as #119 (mergeConditionalStores drops atomic), but applied
to `cmpxchg` -- a distinct LLVM instruction kind not covered by that report.

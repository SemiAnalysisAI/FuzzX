# w105 - SimplifyCFG SinkCommonCodeFromPredecessors merges two `fence` instructions

## Location

`llvm/lib/Transforms/Utils/SimplifyCFG.cpp` - sink-common-insts path
(`SinkCommonCodeFromPredecessors` -> `canSinkInstructions` -> `Instruction::isSameOperationAs` -> `hasSameSpecialState`).

`llvm/lib/IR/Instruction.cpp:956-958` (`hasSameSpecialState` for
`FenceInst`):
```
if (const FenceInst *FI = dyn_cast<FenceInst>(I1))
  return FI->getOrdering() == cast<FenceInst>(I2)->getOrdering() &&
         FI->getSyncScopeID() == cast<FenceInst>(I2)->getSyncScopeID();
```

Two fences whose ordering and syncscope match are reported equivalent.
SimplifyCFG's sink driver therefore happily collapses two `fence` instructions
from mutually-exclusive predecessors into a single fence in the common
successor. This is structurally the same pattern as w122 (sink-seqcst) but
applied to bare `fence` rather than fence-like loads/stores.

This is unsound when there are diverging memory operations between the two
predecessors and the fence reorder-barrier. Originally each predecessor has a
self-contained release sequence `op-A ; fence` (or `op-B ; fence`). After the
sink, the predecessor stores merge through the join PHI into a single store
*after* selecting the value, and only one fence remains. The relative ordering
between the original distinct stores and their per-path fence is collapsed
into a single store -> single fence sequence, removing one of the two
release-acquire pairs that the source program guaranteed.

## Repro

```llvm
target triple = "x86_64-unknown-linux-gnu"

define void @sink_fence(i1 %c, ptr %p, ptr %q, i32 %a, i32 %b) {
entry:
  br i1 %c, label %then, label %else
then:
  store atomic i32 %a, ptr %p release, align 4
  fence acquire
  br label %tail
else:
  store atomic i32 %b, ptr %q release, align 4
  fence acquire
  br label %tail
tail:
  ret void
}
```

## Invocation

```
opt -passes='simplifycfg<sink-common-insts>' -S input.ll
```

## Before / after

Before: each predecessor contains an atomic release store followed by an
acquire fence, giving two distinct release-acquire pairs (depending on which
path is taken).

After:

```
entry:
  br i1 %c, label %then, label %else
then:
  store atomic i32 %a, ptr %p release, align 4
  br label %tail
else:
  store atomic i32 %b, ptr %q release, align 4
  br label %tail
tail:
  fence acquire
  ret void
}
```

The acquire fence is moved past control flow into a position where it no
longer brackets the release stores in their original predecessors. The result
is one acquire fence after the join rather than two acquire fences before the
join. Tools that observe `fence` events directly (Thread Sanitizer,
hardware-level memory ordering analyzers) see fewer fence events than the
source program contains.

The structurally equivalent hoist counterpart (and the all-equal fast path
that produces a fully empty body) is also affected.

## Recommended fix

`canSinkInstructions` / `hoistCommonCodeFromSuccessors` should refuse to
merge `FenceInst` instructions, matching the existing treatment for volatile
accesses and the spirit of w122. A targeted check `if (isa<FenceInst>(I))
return false;` in the sink/hoist guards (alongside the existing volatile
checks) would close the gap. Alternatively `hasSameSpecialState` for
`FenceInst` could return `false` unconditionally to keep the front-end
identity check that genuinely needs full equality (e.g. CSE inside one block)
while disabling cross-block merging.

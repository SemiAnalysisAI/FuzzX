# w57 — SimplifyCFG sink-common-insts merges two `volatile` stores into one

## Location

`llvm/lib/Transforms/Utils/SimplifyCFG.cpp` — sinking common code path
(driver `SinkCommonCodeFromPredecessors`, called when
`sink-common-insts=true`).

`canSinkInstructions` / the underlying identity comparator use
`hasSameSpecialState`, which for `StoreInst` only requires the two stores
to *agree* on `isVolatile`. Therefore two volatile stores with the same
pointer and PHI-able value operands are happily sunk into the common
successor and the two `store volatile` instructions in the original
program are collapsed into **one** `store volatile` in the join block.

This violates the language guarantee that the count and ordering of
volatile accesses is observable and cannot be reduced.

## Repro

```llvm
target triple = "x86_64-unknown-linux-gnu"

define void @sink_volatile(ptr %p, i32 %a, i32 %b, i1 %c) {
entry:
  br i1 %c, label %then, label %else
then:
  store volatile i32 %a, ptr %p, align 4
  br label %tail
else:
  store volatile i32 %b, ptr %p, align 4
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

Before: two `store volatile i32 _, ptr %p, align 4` in mutually-exclusive
predecessor blocks (one volatile observable write per execution; the value
stored depends on the branch but the *source-level* program contains two
distinct volatile statements).

After:
```
entry:
  %a.b = select i1 %c, i32 %a, i32 %b
  store volatile i32 %a.b, ptr %p, align 4
  ret void
```

Only one volatile store remains. The transformation is unsafe for the same
reason as the hoist variant: volatile stores at different source locations
may be observed by debuggers, signal handlers, hardware-watchpoints, etc.
and must not be merged.

## Recommended fix

The sink driver should refuse to sink (or treat as unmergeable) any
instruction `I` with `I->isVolatile()` (or, more conservatively, `!
I->isSimple()`). Same fix as the hoist counterpart.

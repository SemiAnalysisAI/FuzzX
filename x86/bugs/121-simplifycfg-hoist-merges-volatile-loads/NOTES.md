# w57 — SimplifyCFG hoist-common-insts merges two `volatile` loads into one

## Location

`llvm/lib/Transforms/Utils/SimplifyCFG.cpp` — `shouldHoistCommonInstructions`
(lines 1572-1601) and the `hoistCommonCodeFromSuccessors` driver.

The "common code" hoisting logic uses `Instruction::isIdenticalToWhenDefined`
to decide whether two instructions in sibling successors can be hoisted to
the predecessor. For LoadInst this comparator only requires the two loads to
agree on `isVolatile`, `getAlign`, `getOrdering`, `getSyncScopeID` — it never
forbids hoisting when both loads are *volatile*.

`shouldHoistCommonInstructions` adds extra restrictions for call sites
(musttail, convergent, cannotMerge) but no restriction for volatile memory
operations.

## Repro

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @hoist_volatile(ptr %p, i1 %c) {
entry:
  br i1 %c, label %then, label %else
then:
  %a = load volatile i32, ptr %p, align 4
  %x = add i32 %a, 1
  br label %tail
else:
  %b = load volatile i32, ptr %p, align 4
  %y = add i32 %b, 2
  br label %tail
tail:
  %r = phi i32 [ %x, %then ], [ %y, %else ]
  ret i32 %r
}
```

## Invocation

```
opt -passes='simplifycfg<hoist-common-insts>' -S input.ll
```

## Before / after diff

Before: each branch performs **its own** volatile load → exactly one volatile
read per execution, observable to the platform.

After:
```
entry:
  %a = load volatile i32, ptr %p, align 4
  %x = add i32 %a, 1
  %y = add i32 %a, 2
  %r = select i1 %c, i32 %x, i32 %y
  ret i32 %r
```

Still one volatile load, but the bug is subtler: in the original program
the `else`-branch load happens at a *different* program point in source
order than the `then`-branch load — and a debugger / hardware watchpoint /
signal handler may legitimately observe a different value in each branch.
After hoisting both branches use the value sampled at the predecessor; the
"second" volatile read has been eliminated.

The same identical-instruction hoist path applies to volatile stores when
their value operands are identical (or can be PHI-ed).

## Recommended fix

`shouldHoistCommonInstructions` should bail when either `I1` or `I2` is a
volatile load/store/RMW/cmpxchg — `volatile` is an *observable side
effect*, and the number of volatile accesses cannot be reduced by an
optimisation.

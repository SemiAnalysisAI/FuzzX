# w57 — SimplifyCFG hoist-common-insts merges two `seq_cst` atomic loads into one

## Location

`llvm/lib/Transforms/Utils/SimplifyCFG.cpp` — `shouldHoistCommonInstructions`
and `hoistCommonCodeFromSuccessors`. The identity check
(`isIdenticalToWhenDefined`) requires only same volatile/align/ordering/sync —
so two identical `seq_cst` loads are happily hoisted.

This is **wrong for `seq_cst`** (and for any monotonic-or-stronger atomic)
because each such load participates in the global *sequenced-before / happens-
before* relation. Reducing the number of `seq_cst` loads can change the set of
*coherence-observable* outcomes of concurrent programs.

## Repro

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @atomic_hoist(ptr %p, i1 %c) {
entry:
  br i1 %c, label %then, label %else
then:
  %a = load atomic i32, ptr %p seq_cst, align 4
  %x = add i32 %a, 1
  br label %tail
else:
  %b = load atomic i32, ptr %p seq_cst, align 4
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

## Before / after

Before: each branch performs **its own** `load atomic seq_cst` → exactly one
`seq_cst` read per execution at a *branch-specific source location* (so a
debugger or sanitizer can observe the values at distinct program points; the
synchronization participates twice in the `S` total order across both
possible executions).

After:
```
entry:
  %a = load atomic i32, ptr %p seq_cst, align 4
  %x = add i32 %a, 1
  %y = add i32 %a, 2
  %r = select i1 %c, i32 %x, i32 %y
  ret i32 %r
```

The hoisted `load atomic seq_cst` is now executed *unconditionally* — before
the branch. The seq_cst load was previously *conditional*. Adding new
`seq_cst` operations to a thread that previously did not execute one alters
the global total order S of seq_cst operations across all threads. By the
C++ memory model, this can cause new sequenced reads of other threads'
writes that the source program did not permit.

## Recommended fix

`shouldHoistCommonInstructions` should refuse to hoist any
`!I->isUnordered()` load/store, and refuse to hoist *any* atomic operation
where the hoist would change the set of program paths that execute it
(speculation of seq_cst, release, acq_rel and acquire is unsound).
